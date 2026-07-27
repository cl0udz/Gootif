# Gootif Server — Deployment & Hardening

The PHP API lives at the webroot of `gootif.jianwei.me` (Apache 2.4 / Ubuntu).
This doc records what belongs on the web server, what must never be there, and
how to verify the deployment from the outside.

## What to deploy under the webroot

Only these paths should exist on the web server:

```
webroot/
├── .htaccess          # routes + HTTPS/HSTS + deny rules
├── config.php         # copied from config.example.php, real keys — NEVER in git
├── db.php
├── api/
│   ├── notifications.php
│   └── services.php
└── data/              # only if DB_PATH stays inside the webroot (see below)
    └── .htaccess      # "Require all denied"
```

**Never upload `agents/`.** It belongs on the machine that runs the cron jobs
(it contains `.env` with the service key and `google-mcp.json` with OAuth
secrets). Apache does *not* block `.env` files by default — only `.ht*`.
The root `.htaccess` forbids `agents/` and all dotfiles as a safety net, but
the real protection is not putting secrets on the web server at all.

## Database location

Preferred: keep the SQLite DB **outside** the webroot so it can never be
served over HTTP regardless of Apache config. In `config.php`:

```php
define('DB_PATH', dirname(__DIR__) . '/gootif-data/gootif.db');
```

When moving an existing DB, move all three files while the API is idle:
`gootif.db`, `gootif.db-wal`, `gootif.db-shm` (WAL companions).

If the DB stays inside the webroot, two independent layers block it:
the root `.htaccess` (`RewriteRule ^data/ - [F,L]`) and `data/.htaccess`
(`Require all denied`).

## Apache requirements

- `AllowOverride FileInfo AuthConfig` (or `All`) for the webroot, so both
  `.htaccess` files are honored. **If `.htaccess` were ignored, API routes
  would 404 — so a working API implies the deny rules are active too.**
- `mod_rewrite` enabled (routing depends on it).
- `mod_headers` enabled for HSTS / nosniff (rules are `<IfModule>`-wrapped,
  so nothing breaks without it — the headers are just silently missing).
- PHP handler active. If PHP is ever disabled, `config.php` would be served
  as source, keys included — another reason to keep keys out of the webroot
  where possible, and to re-run the verification below after server changes.
- `display_errors=Off` in production php.ini (`db.php` also forces it off
  and installs a generic-500 exception handler as a second layer).

## HTTPS

The vhost 301-redirects HTTP→HTTPS, and the root `.htaccess` does the same
(portable across hosts). HSTS (`Strict-Transport-Security: max-age=31536000`)
pins clients to HTTPS afterwards. iOS ATS requires TLS regardless — clients
should only ever be configured with `https://` URLs.

## Verifying a deployment

Run from any machine; expected results on a healthy deployment:

```bash
BASE=https://gootif.jianwei.me
curl -s -o /dev/null -w '%{http_code}\n' $BASE/agents/.env          # 404 (not deployed) or 403
curl -s -o /dev/null -w '%{http_code}\n' $BASE/data/gootif.db       # 403 (or 404 if DB moved out)
curl -s -o /dev/null -w '%{http_code}\n' $BASE/.env                 # 403/404, never 200
curl -s -o /dev/null -w '%{http_code}\n' $BASE/config.php           # 200 with 0 bytes (PHP executes, no output)
curl -s -o /dev/null -w '%{http_code} %{redirect_url}\n' http://gootif.jianwei.me/api/services   # 301 → https
curl -s $BASE/api/services | head -c 100                            # {"error":"Unauthorized"} without a key
curl -sI $BASE/api/services | grep -i strict-transport              # HSTS header present
```

Last verified against the live server: 2026-07-27 (agents/ absent, data/
blocked, PHP executing, HTTP→HTTPS redirect active; HSTS added in this
revision — re-verify after deploying).

## API

See [API.md](API.md). Request limits enforced by `api/notifications.php`:
`service` ≤ 100 B, `title` ≤ 500 B, `body` ≤ 64 KB, `metadata` ≤ 1 MB encoded
and must be a JSON object. Generous on purpose — agent reports ride in
`metadata` — while still bounding what a leaked service key could stuff into
the DB.
