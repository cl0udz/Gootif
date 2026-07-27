# Gootif Server

Self-hosted push notification hub: home-lab services and scheduled Claude
agents post to this tiny PHP + SQLite API, and native client apps render the
feed — from one-line cron alerts up to rich, structured agent reports
(pre-market briefs, inbox digests, LLM security news).

This repo is the **server + agents**. The clients live in their own repos:

- [Gootif-iOS](https://github.com/cl0udz/Gootif-iOS) — SwiftUI iPhone app
- [Gootif-MacOS](https://github.com/cl0udz/Gootif-MacOS) — SwiftUI Mac app
- [Gootif-Windows](https://github.com/cl0udz/Gootif-Windows) — Windows app

```
┌─────────────────┐     POST /api/notifications      ┌──────────────────┐
│ services & cron │  ────────────────────────────▶   │  this repo       │
│ (X-Service-Key) │                                  │  PHP + SQLite    │
└─────────────────┘                                  └──────────────────┘
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
| `api/` | Endpoints: `POST/GET/DELETE /api/notifications`, `GET /api/services` — contract in [API.md](API.md) |
| `config.example.php` | Copy to `config.php`, fill in the two keys (service = write, app = read/delete) |
| `agents/` | Headless Claude Code agents run by cron (`run.sh` → `runner.py` → `claude -p`), posting structured reports. See [agents/README.md](agents/README.md) and [agents/REPORTS.md](agents/REPORTS.md) |
| [DEPLOYMENT.md](DEPLOYMENT.md) | Webroot layout, hardening, and outside-in verification probes |

## Quick start

```bash
cp config.example.php config.php
# fill in two random keys:  php -r "echo bin2hex(random_bytes(24)), PHP_EOL;"
```

Upload everything **except `agents/`** (and never a real `.env`) to an Apache
webroot with PHP 8+, mod_rewrite, and HTTPS — details in
[DEPLOYMENT.md](DEPLOYMENT.md).

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

## Secrets

Real keys never live in git. `config.php`, `agents/.env`, and
`agents/google-mcp.json` are gitignored — copy each from its `.example`
sibling and fill in your values.
