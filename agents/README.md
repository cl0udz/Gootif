# Gootif Claude Agents

Run self-contained Claude Code agents headlessly on your server (using your
Claude subscription via `claude -p`), then post each agent's report to Gootif so
it's readable on your phone.

```
agents/
├── runner.py            # runs an agent, extracts JSON, posts to Gootif
├── run.sh               # cron wrapper (loads .env)
├── agents.json          # agent definitions (service, kind, model, tools, prompt)
├── prompts/             # one prompt per agent
├── google-mcp.example.json
├── crontab.example
├── .env.example
├── REPORTS.md           # agent → app data contract
└── logs/                # created on first run
```

## Pipeline at a glance

```
cron → run.sh <agent> → claude -p (your subscription) → JSON report
     → POST /api/notifications (X-Service-Key) → SQLite → iOS app renders it
```

Each agent = a Gootif **service**; each run = one **report** (a notification with
a structured `metadata.payload`). The app renders a native view per `kind`.

## One-time setup

1. **Claude Code on the server, logged into your subscription.**
   Install Claude Code, then `claude` once interactively and complete the
   subscription login. Verify headless works:
   ```bash
   claude -p "say hi" --output-format json
   ```
   Note the login lives in `~/.claude`, so cron jobs must run as the same user
   with `HOME` set (handled via `.env`).

2. **Config & env.**
   ```bash
   cp .env.example .env        # set HOME, CLAUDE_BIN, GOOTIF_SERVICE_KEY
   chmod +x run.sh runner.py
   ./run.sh premarket-brief --dry-run   # smoke test, prints payload, no POST
   ```

3. **Schedule.** Edit paths in `crontab.example`, then `crontab crontab.example`.
   `CRON_TZ=America/Los_Angeles` makes `0 6 * * *` fire at 6am Pacific.

## Agents

| name              | when (default)   | tools                  |
|-------------------|------------------|------------------------|
| `premarket-brief` | 6:00 AM PT daily | WebSearch, WebFetch    |
| `inbox-digest`    | 7:00 AM daily    | Google Workspace (read)|
| `llm-security`    | 8:00 AM weekdays | WebSearch, WebFetch    |

Change a schedule by editing `crontab.example`; change behavior by editing the
file in `prompts/`.

## Agent 2 — Gmail + Calendar, strictly read-only

Two independent layers keep it read-only (defense in depth):

1. **OAuth scopes — the real guarantee.** When you authorize the Google
   Workspace MCP, grant ONLY:
   - `https://www.googleapis.com/auth/gmail.readonly`
   - `https://www.googleapis.com/auth/calendar.readonly`

   Google itself will reject any send/modify/delete call made with these scopes.
   Set this on the OAuth consent screen / client in Google Cloud Console and
   confirm the consent screen lists only "read" access when you authorize.

2. **Tool allowlist.** `agents.json → inbox-digest.allowed_tools` lists only read
   tools. The runner never passes `--dangerously-skip-permissions`, so any tool
   not on the list is denied in headless mode. The prompt also forbids writes.

Setup:
```bash
cp google-mcp.example.json google-mcp.json
# edit: OAuth client id/secret, your email, and match tool names to your server
```
Then do the MCP's one-time OAuth flow (it opens a browser / prints a URL) and
grant the read-only scopes. Tokens are cached so cron runs unattended.

> **Multiple inboxes:** authorize each Google account once; most Workspace MCP
> servers let you pass an account per request, or run one MCP server per account
> (duplicate the block in `google-mcp.json` with a different name and add its
> read tools to `allowed_tools`).

> Tool names in `agents.json`/`google-mcp.example.json` are for the common
> `workspace-mcp` server. If you use a different one, update both files to match
> its actual read-tool names (`./run.sh inbox-digest --dry-run` will tell you if
> a tool was denied).

## How structured output is made reliable
- Prompts instruct the agent to return **only** a JSON object `{title, body,
  priority, payload}`.
- `runner.py` runs with `--output-format json`, reads `.result`, and extracts the
  JSON (tolerating fences/prose via brace-matching).
- On a parse failure it retries once with a strict nudge; if it still fails it
  posts a **high-priority failure notification** so you see the breakage on your
  phone instead of silent nothing.

## Delivery
v1 is pull: reports appear when you open/refresh the app. APNs push is a clean
follow-up (the server already stores everything; add a device-token table + an
APNs send after each insert in `../api/notifications.php`).

## Troubleshooting
- `claude CLI not found` → set `CLAUDE_BIN` in `.env` (`which claude`).
- Empty/auth errors from claude in cron → `HOME` not set; fix in `.env`.
- `GOOTIF_SERVICE_KEY not set` → fill it in `.env` (value in `../API.md`).
- A tool was denied → its name isn't in `allowed_tools` (or scope too narrow).
- Inspect a run without posting: `./run.sh <agent> --dry-run`.
- Logs: `logs/<agent>.log` (add `>> logs/x.log 2>&1` in cron if you want files).
