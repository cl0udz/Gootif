#!/usr/bin/env python3
"""
Gootif agent runner.

Runs a self-contained Claude Code agent headlessly (`claude -p`, using your
subscription login), extracts its structured JSON output, and posts it to the
Gootif notifications API so it shows up in the iOS app.

Usage:
    ./runner.py <agent-name>            # run one agent defined in agents.json
    ./runner.py <agent-name> --dry-run  # run + print payload, don't POST

Environment:
    GOOTIF_URL          default https://gootif.jianwei.me
    GOOTIF_SERVICE_KEY  service key for POST /api/notifications (required to post)
    CLAUDE_BIN          path to the claude CLI (default: "claude")

Exit codes: 0 ok, 1 config/usage error, 2 agent/parse failure (a failure
notification is still posted so you see it on your phone).
"""
import argparse
import json
import os
import subprocess
import sys
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEFAULT_URL = "https://gootif.jianwei.me"
RUN_TIMEOUT_SEC = 900  # 15 min hard cap per agent


def load_agents() -> dict:
    cfg = HERE / "agents.json"
    if not cfg.exists():
        die(1, f"missing config: {cfg}")
    return json.loads(cfg.read_text())


def die(code: int, msg: str):
    print(f"[runner] error: {msg}", file=sys.stderr)
    sys.exit(code)


def log(msg: str):
    ts = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    print(f"[runner {ts}] {msg}", file=sys.stderr)


# --- claude invocation -------------------------------------------------------

def run_claude(agent: dict, extra_nudge: str = "") -> str:
    """Run claude -p and return the final assistant text (the .result field)."""
    prompt_path = HERE / agent["prompt"]
    if not prompt_path.exists():
        die(1, f"missing prompt file: {prompt_path}")
    prompt = prompt_path.read_text()
    if extra_nudge:
        prompt += "\n\n" + extra_nudge

    claude = os.environ.get("CLAUDE_BIN", "claude")
    cmd = [
        claude, "-p", prompt,
        "--output-format", "json",
        "--model", agent.get("model", "claude-opus-4-8"),
    ]

    allowed = agent.get("allowed_tools") or []
    if allowed:
        # Allowlist exactly the read-only tools each agent needs. Anything not
        # listed is denied in headless mode (we never pass --dangerously-skip-permissions).
        cmd += ["--allowedTools", *allowed]

    mcp = agent.get("mcp_config")
    if mcp:
        mcp_path = HERE / mcp
        if not mcp_path.exists():
            die(1, f"missing mcp config: {mcp_path} (see google-mcp.example.json)")
        cmd += ["--mcp-config", str(mcp_path), "--strict-mcp-config"]

    log(f"invoking: {claude} -p <prompt> --model {agent.get('model')} "
        f"tools={allowed or 'none'} mcp={mcp or 'none'}")
    try:
        proc = subprocess.run(
            cmd, capture_output=True, text=True, timeout=RUN_TIMEOUT_SEC, cwd=HERE,
        )
    except subprocess.TimeoutExpired:
        raise RuntimeError(f"claude timed out after {RUN_TIMEOUT_SEC}s")
    except FileNotFoundError:
        die(1, f"claude CLI not found (CLAUDE_BIN={claude}). Is Claude Code installed & logged in?")

    if proc.returncode != 0:
        raise RuntimeError(f"claude exited {proc.returncode}: {proc.stderr[-800:]}")

    # --output-format json wraps the run; .result holds the final assistant text.
    try:
        envelope = json.loads(proc.stdout)
        if isinstance(envelope, dict) and envelope.get("is_error"):
            raise RuntimeError(f"claude reported error: {envelope.get('result', '')[:800]}")
        return envelope["result"] if isinstance(envelope, dict) and "result" in envelope else proc.stdout
    except json.JSONDecodeError:
        return proc.stdout  # fall back to raw stdout


# --- JSON extraction ---------------------------------------------------------

def extract_json(text: str) -> dict:
    """Pull a JSON object out of the agent's final message, tolerating prose/fences."""
    text = text.strip()
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        pass
    # strip ```json ... ``` fences
    if "```" in text:
        for chunk in text.split("```"):
            chunk = chunk.strip()
            if chunk.startswith("json"):
                chunk = chunk[4:].strip()
            if chunk.startswith("{"):
                try:
                    return json.loads(chunk)
                except json.JSONDecodeError:
                    continue
    # brace-match the first balanced {...}
    start = text.find("{")
    if start >= 0:
        depth = 0
        in_str = False
        esc = False
        for i in range(start, len(text)):
            c = text[i]
            if in_str:
                if esc:
                    esc = False
                elif c == "\\":
                    esc = True
                elif c == '"':
                    in_str = False
            else:
                if c == '"':
                    in_str = True
                elif c == "{":
                    depth += 1
                elif c == "}":
                    depth -= 1
                    if depth == 0:
                        return json.loads(text[start:i + 1])
    raise ValueError("no JSON object found in agent output")


# --- Gootif posting ----------------------------------------------------------

def post_notification(service: str, title: str, body: str, priority: str, metadata: dict):
    url = os.environ.get("GOOTIF_URL", DEFAULT_URL).rstrip("/") + "/api/notifications"
    key = os.environ.get("GOOTIF_SERVICE_KEY")
    if not key:
        die(1, "GOOTIF_SERVICE_KEY not set")
    payload = json.dumps({
        "service": service, "title": title, "body": body,
        "priority": priority, "metadata": metadata,
    }).encode()
    req = urllib.request.Request(url, data=payload, method="POST", headers={
        "Content-Type": "application/json", "X-Service-Key": key,
    })
    with urllib.request.urlopen(req, timeout=30) as resp:
        return resp.status


def post_failure(service: str, err: str):
    try:
        post_notification(
            service, f"⚠️ {service} agent failed",
            err[:500], "high", {"kind": "agent_error"},
        )
        log("posted failure notification")
    except Exception as e:  # noqa: BLE001 - best effort
        log(f"could not post failure notification: {e}")


# --- main --------------------------------------------------------------------

def build_and_post(agent: dict, result_text: str, dry_run: bool):
    obj = extract_json(result_text)
    title = obj.get("title") or agent["service"]
    body = obj.get("body") or obj.get("summary") or ""
    priority = obj.get("priority") if obj.get("priority") in ("low", "medium", "high") else "medium"
    payload = obj.get("payload")
    if payload is None:
        raise ValueError("agent output missing 'payload'")
    metadata = {"kind": agent["kind"], "payload": payload}

    if dry_run:
        print(json.dumps({
            "service": agent["service"], "title": title, "body": body,
            "priority": priority, "metadata": metadata,
        }, indent=2))
        return
    status = post_notification(agent["service"], title, body, priority, metadata)
    log(f"posted report to Gootif (HTTP {status})")


def main():
    ap = argparse.ArgumentParser(description="Run a Gootif Claude agent and post its report.")
    ap.add_argument("agent", help="agent name as defined in agents.json")
    ap.add_argument("--dry-run", action="store_true", help="print payload, do not POST")
    args = ap.parse_args()

    agents = load_agents()
    if args.agent not in agents:
        die(1, f"unknown agent '{args.agent}'. known: {', '.join(agents)}")
    agent = agents[args.agent]

    try:
        result_text = run_claude(agent)
        try:
            build_and_post(agent, result_text, args.dry_run)
        except (ValueError, KeyError) as parse_err:
            log(f"first parse failed ({parse_err}); retrying once with strict nudge")
            nudge = ("Your previous response was not valid. Respond with ONLY a single "
                     "JSON object matching the schema — no prose, no markdown fences.")
            result_text = run_claude(agent, extra_nudge=nudge)
            build_and_post(agent, result_text, args.dry_run)
    except Exception as e:  # noqa: BLE001
        log(f"FAILED: {e}")
        if not args.dry_run:
            post_failure(agent["service"], str(e))
        sys.exit(2)


if __name__ == "__main__":
    main()
