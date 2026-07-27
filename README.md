# Gootif

Self-hosted push notification hub for your own ecosystem: home-lab services and
scheduled Claude agents post to a tiny PHP API, and a native SwiftUI iOS app
renders them — from one-line cron alerts up to rich, structured agent reports
(pre-market briefs, inbox digests, LLM security news).

```
┌─────────────────┐     POST /api/notifications      ┌──────────────────┐
│ services & cron │  ────────────────────────────▶   │  PHP + SQLite    │
│ (X-Service-Key) │                                  │  gootif.jianwei… │
└─────────────────┘                                  └──────────────────┘
┌─────────────────┐              ▲                            │
│ Claude agents   │──────────────┘                GET (X-API-Key)
│ (claude -p cron)│                                           ▼
└─────────────────┘                                  ┌──────────────────┐
                                                     │   Gootif iOS     │
                                                     │   (SwiftUI)      │
                                                     └──────────────────┘
```

## Parts

| Path | What it is |
|------|------------|
| `Gootif/` | iOS app (SwiftUI, iOS 18). Tabs: Recent, Services, Settings. Rich report renderers selected by `metadata.kind` |
| `server/` | PHP API + SQLite. Two keys: service key (write), app key (read/delete). See [server/README.md](server/README.md) for deployment & hardening |
| `server/api/` | Endpoints: `POST/GET/DELETE /api/notifications`, `GET /api/services` — contract in [server/API.md](server/API.md) |
| `server/agents/` | Headless Claude Code agents run by cron (`run.sh` → `runner.py` → `claude -p`), posting structured reports. See [server/agents/README.md](server/agents/README.md) and [REPORTS.md](server/agents/REPORTS.md) |

## Quick start

### iOS app

```bash
brew install xcodegen
cp Gootif/Services/Secrets.swift.example Gootif/Services/Secrets.swift
# edit Secrets.swift: set the APP_KEY from your server config.php
xcodegen generate
open Gootif.xcodeproj
```

The server base URL lives in `Gootif/Services/APIClient.swift`; the API key can
also be overridden at runtime in the app's Settings tab.

### Server

```bash
cp server/config.example.php server/config.php
# fill in two random keys:  php -r "echo bin2hex(random_bytes(24)), PHP_EOL;"
```

Upload `server/` — **without** `agents/`, and never a real `.env` — to an
Apache webroot with PHP 8+, mod_rewrite, and HTTPS. Layout, Apache
requirements, and outside-in verification probes: [server/README.md](server/README.md).

### Sending a notification

```bash
curl -X POST https://your-host/api/notifications \
  -H "Content-Type: application/json" \
  -H "X-Service-Key: YOUR_SERVICE_KEY" \
  -d '{"service":"home-server","title":"Disk alert","body":"Root at 90%","priority":"high"}'
```

### Agents (optional)

On any machine with Claude Code installed and logged in:

```bash
cd server/agents
cp .env.example .env            # fill in GOOTIF_SERVICE_KEY etc.
./run.sh premarket-brief --dry-run
crontab -e                      # see crontab.example
```

Agents post normal notifications whose `metadata` carries `kind` +
a structured `payload`; the app picks a native renderer per kind
(`news_feed`, `task_digest`, `security_feed`) and falls back to the generic
detail view for unknown kinds.

## Secrets

Real keys never live in git. `config.php`, `Secrets.swift`, `agents/.env`, and
`agents/google-mcp.json` are gitignored — copy each from its `.example`
sibling and fill in your values.
