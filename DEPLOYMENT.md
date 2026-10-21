# Gootif Server — Deployment & Hardening

`gootif.jianwei.me` runs the `gootif` binary behind Apache 2.4 on Ubuntu.
Apache terminates TLS and proxies `/api/` to the API on `127.0.0.1:18090`;
nothing is served from disk. A second process delivers APNs pushes.

## Build

```bash
cd server
cargo build --release --locked
cargo test
```

Building on the server needs only `rustup` (minimal profile) and `gcc`.
SQLite is compiled in (`rusqlite` `bundled`) and TLS is rustls, so the binary
has no runtime library dependencies beyond glibc.

## Install

```bash
sudo install -m 755 server/target/release/gootif /usr/local/bin/gootif

sudo install -d -m 750 -o root -g www-data /etc/gootif
sudo install -m 640 -o root -g www-data deploy/gootif.env.example /etc/gootif/gootif.env
sudoedit /etc/gootif/gootif.env          # keys: openssl rand -hex 24
# APNs: put AuthKey_*.p8 in /etc/gootif (root:www-data 640) and set APNS_*.

sudo install -m 644 deploy/gootif-api.service deploy/gootif-push.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now gootif-api gootif-push
```

Both units run as `www-data` with `ProtectSystem=strict`. The only writable
path is `StateDirectory=gootif` → `/var/lib/gootif`, so `DB_PATH` must point
there (default `/var/lib/gootif/gootif.db`). The DB is SQLite in WAL mode;
when moving it, stop both units and move all three files
(`gootif.db`, `gootif.db-wal`, `gootif.db-shm`).

The push worker takes an exclusive lock on `DB_PATH.push.lock`, so a second
worker refuses to start. With `APNS_ENABLED=0` it exits immediately and
notifications are stored without being pushed.

Upgrading: build, `install` the new binary over `/usr/local/bin/gootif`, then
`sudo systemctl restart gootif-api gootif-push`.

## Apache

`deploy/apache-gootif.conf` is the whole vhost: HTTP→HTTPS redirect, TLS,
`ProxyPass /api/ http://127.0.0.1:18090/api/`, HSTS + `nosniff`, and an
empty `DocumentRoot` with `Options -Indexes`. Requires `mod_proxy`,
`mod_proxy_http`, `mod_rewrite`, `mod_ssl`, and (for the headers) `mod_headers`.

```bash
sudo install -d -m 755 /var/www/gootif-public
sudo install -m 644 deploy/apache-gootif.conf /etc/apache2/sites-available/gootif.conf
sudo a2ensite gootif && sudo apache2ctl configtest && sudo systemctl reload apache2
```

**Never put `agents/` or any secret on the web server.** `agents/` belongs
on the machine that runs the cron jobs (it contains `.env` with the service
key and `google-mcp.json` with OAuth secrets). The API is a separate process,
so the webroot holds no code and no keys at all.

## HTTPS

The vhost 301-redirects HTTP→HTTPS. HSTS (`max-age=31536000`) pins clients to
HTTPS afterwards. iOS ATS requires TLS regardless — clients should only ever be
configured with `https://` URLs.

## Verifying a deployment

Run from any machine; expected results on a healthy deployment:

```bash
BASE=https://gootif.jianwei.me
curl -s -o /dev/null -w '%{http_code}\n' $BASE/                     # 403/404 — no directory listing
curl -s -o /dev/null -w '%{http_code}\n' $BASE/.env                 # 403/404, never 200
curl -s -o /dev/null -w '%{http_code}\n' $BASE/agents/.env          # 403/404, never 200
curl -s -o /dev/null -w '%{http_code} %{redirect_url}\n' http://gootif.jianwei.me/api/services   # 301 → https
curl -s $BASE/api/services | head -c 100                            # {"error":"Unauthorized"} without a key
curl -sI $BASE/api/services | grep -i strict-transport              # HSTS header present
```

On the server:

```bash
systemctl is-active gootif-api gootif-push
journalctl -u gootif-api -u gootif-push -n 20
```

## API

See [API.md](API.md). Request limits enforced by the API:
`service` ≤ 100 B, `title` ≤ 500 B, `body` ≤ 64 KB, `metadata` ≤ 1 MB encoded
and must be a JSON object; `POST /api/devices` bodies ≤ 4 KB. Generous on
purpose — agent reports ride in `metadata` — while still bounding what a
leaked service key could stuff into the DB.
