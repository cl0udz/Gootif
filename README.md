# Gootif Server

Self-hosted push notification hub: home-lab services and scheduled Claude
agents post to this small Rust + SQLite API, and native client apps render the
feed — from one-line cron alerts up to rich, structured agent reports
(pre-market briefs, inbox digests, LLM security news). New notifications are
pushed to iOS/macOS devices through APNs.

This repo is the **server + agents**. The clients live in their own repos:

- [Gootif-iOS](https://github.com/cl0udz/Gootif-iOS) — SwiftUI iPhone app
- [Gootif-MacOS](https://github.com/cl0udz/Gootif-MacOS) — SwiftUI Mac app
- [Gootif-Windows](https://github.com/cl0udz/Gootif-Windows) — Windows app

```
┌─────────────────┐     POST /api/notifications      ┌──────────────────┐   APNs   ┌──────────┐
│ services & cron │  ────────────────────────────▶   │  gootif (Rust)   │ ───────▶ │ iPhone / │
│ (X-Service-Key) │                                  │  + SQLite        │          │ Mac      │
└─────────────────┘                                  └──────────────────┘          └──────────┘
┌─────────────────┐              ▲                            │
│ Claude agents   │──────────────┘                GET (X-API-Key)
│ (claude -p cron)│                                           ▼
└─────────────────┘                                  ┌──────────────────┐
                                                     │ iOS / macOS /    │
                                                     │ Windows clients  │
                                                     └──────────────────┘
```

## Layout

| Path | What it is |
|------|------------|
| `server/` | The `gootif` binary: `gootif serve` (HTTP API) and `gootif push-worker` (APNs delivery). Contract in [API.md](API.md) |
| `deploy/` | systemd units, Apache reverse-proxy vhost, and `gootif.env.example` |
| `agents/` | Headless Claude Code agents run by cron (`run.sh` → `runner.py` → `claude -p`), posting structured reports. See [agents/README.md](agents/README.md) and [agents/REPORTS.md](agents/REPORTS.md) |
| [DEPLOYMENT.md](DEPLOYMENT.md) | Build, install, hardening, and outside-in verification probes |

## Quick start

```bash
cd server && cargo build --release        # → target/release/gootif
cp ../deploy/gootif.env.example gootif.env
# fill in two random keys (openssl rand -hex 24) and point DB_PATH somewhere writable
GOOTIF_CONFIG=gootif.env ./target/release/gootif serve --listen 127.0.0.1:18090
```

Production runs both processes under systemd behind an HTTPS reverse proxy —
details in [DEPLOYMENT.md](DEPLOYMENT.md).

Send a notification:

```bash
curl -X POST https://your-host/api/notifications \
  -H "Content-Type: application/json" \
  -H "X-Service-Key: YOUR_SERVICE_KEY" \
  -d '{"service":"home-server","title":"Disk alert","body":"Root at 90%","priority":"high"}'
```

Run agents from any machine with Claude Code installed and logged in:

```bash
cd agents
cp .env.example .env            # fill in GOOTIF_SERVICE_KEY etc.
./run.sh premarket-brief --dry-run
crontab -e                      # see crontab.example
```

Agents post normal notifications whose `metadata` carries `kind` + a
structured `payload`; clients pick a native renderer per kind (`news_feed`,
`task_digest`, `security_feed`) and fall back to a generic detail view for
unknown kinds.

## Tests

```bash
cd server && cargo test
```

Covers the HTTP contract (auth, validation, paging, devices) against an
in-memory database, plus push-queue bookkeeping and the APNs payload shape.

## History

The server was originally PHP. On 2026-10-21 it was replaced by this Rust
binary in response to php2shell (CVE-2026-109618/109619). The HTTP contract
and the SQLite schema are unchanged, so clients and existing databases work
as-is. Before cut-over both implementations ran side by side on copies of the
production DB and agreed on all 53 request/response and DB-state checks, and
both push workers were checked against APNs sandbox and production.

## Secrets

Real keys never live in git. `gootif.env`, `agents/.env`, and
`agents/google-mcp.json` are gitignored — copy each from its `.example`
sibling and fill in your values.
