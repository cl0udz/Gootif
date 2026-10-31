# Gootif Push Notification API

## Endpoint

```
POST https://gootif.jianwei.me/api/notifications
```

## Authentication

Include the service API key in the request header:

```
X-Service-Key: YOUR_SERVICE_KEY
```

## Request

**Content-Type:** `application/json`

### Parameters

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `service` | string | **Yes** | — | Identifier for your service (e.g. `home-server`, `nas-backup`, `pi-monitor`) |
| `title` | string | **Yes** | — | Short summary of the notification |
| `body` | string | No | `""` | Detailed message content |
| `priority` | string | No | `"medium"` | One of: `low`, `medium`, `high` |
| `metadata` | object | No | `{}` | Arbitrary key-value pairs for extra context. An http(s) `url` (or `link`, `href`, `buy_url`, `product_url`) of at most 1024 bytes is included in the push, and tapping the push opens it directly |

### Priority Guidelines

- **high** — Requires immediate attention (service down, disk full, security alert)
- **medium** — Notable event (backup completed, update available, threshold warning)
- **low** — Informational (cron job ran, periodic status report)

## Response

**Status:** `201 Created`

```json
{
  "id": "6603fa0b35ca8b9a48407013c397b1bd",
  "service": "home-server",
  "title": "Disk Alert",
  "body": "Root partition at 90% capacity",
  "priority": "high",
  "metadata": {
    "disk": "/dev/sda1",
    "usage": "90%"
  },
  "created_at": "2026-04-01T17:09:55Z"
}
```

## Examples

### Minimal

```bash
curl -X POST https://gootif.jianwei.me/api/notifications \
  -H "Content-Type: application/json" \
  -H "X-Service-Key: YOUR_SERVICE_KEY" \
  -d '{"service":"my-service","title":"Task completed"}'
```

### Full

```bash
curl -X POST https://gootif.jianwei.me/api/notifications \
  -H "Content-Type: application/json" \
  -H "X-Service-Key: YOUR_SERVICE_KEY" \
  -d '{
    "service": "nas-backup",
    "title": "Nightly backup finished",
    "body": "Backed up 142 files (3.2 GB) to /mnt/backup/2026-04-01",
    "priority": "low",
    "metadata": {
      "files": "142",
      "size_gb": "3.2",
      "duration_sec": "847",
      "destination": "/mnt/backup/2026-04-01"
    }
  }'
```

### Python

```python
import requests

requests.post(
    "https://gootif.jianwei.me/api/notifications",
    headers={"X-Service-Key": "YOUR_SERVICE_KEY"},
    json={
        "service": "pi-monitor",
        "title": "Temperature warning",
        "body": "CPU temperature reached 78°C",
        "priority": "high",
        "metadata": {"cpu_temp": "78", "gpu_temp": "65"}
    }
)
```

### Shell Script (one-liner for cron jobs)

```bash
# Send a notification after a cron job
gootif() {
  curl -s -X POST https://gootif.jianwei.me/api/notifications \
    -H "Content-Type: application/json" \
    -H "X-Service-Key: YOUR_SERVICE_KEY" \
    -d "{\"service\":\"$1\",\"title\":\"$2\",\"body\":\"$3\",\"priority\":\"${4:-medium}\"}"
}

# Usage:
gootif "cron" "DB backup done" "Completed in 12 minutes" "low"
```

## Service Naming

Use a consistent, lowercase, hyphenated identifier for each service. This groups notifications in the app.

Suggested names:
- `home-server` — general server alerts
- `nas-backup` — NAS/backup tasks
- `pi-monitor` — Raspberry Pi monitoring
- `docker` — container events
- `cron` — scheduled job results
- `security` — auth failures, intrusion alerts

## Limits

`service` ≤ 100 bytes, `title` ≤ 500 bytes, `body` ≤ 64 KB, `metadata` must be
a JSON object and encode to ≤ 1 MB. An unknown `priority` falls back to `medium`.

## Client endpoints (X-API-Key)

The apps authenticate with the read/delete key instead:

```
X-API-Key: YOUR_APP_KEY
```

| Method & path | Result |
|---------------|--------|
| `GET /api/notifications` | Newest first. Query: `service`, `since` (ISO timestamp, exclusive), `limit` (default 50, max 200), `offset` |
| `GET /api/notifications/{id}` | One notification, or `404` |
| `DELETE /api/notifications/{id}` | `204`, or `404` if it does not exist |
| `GET /api/services` | `[{"name", "notification_count", "last_notification_at"}]`, most recent first |
| `POST /api/devices` | Register an APNs device token (below) |

`metadata` is always returned as a JSON object.

### Device registration

```json
{
  "installation_id": "3f2b8c1e-0000-4000-8000-0000000000aa",
  "platform": "ios",
  "topic": "com.goosheys.gootif",
  "environment": "production",
  "token": "<hex APNs device token>"
}
```

- `platform` is `ios` or `macos`, and `topic` must be that platform's bundle ID.
- `environment` is `development` (APNs sandbox) or `production`.
- `{"installation_id": "...", "enabled": false}` stops pushes to that installation.
- Returns `{"enabled": true|false}`, or `503` when push is not configured on the server.
- The body may be at most 4 KB.

## Error Responses

All errors are JSON: `{"error": "..."}`.

| Status | Meaning |
|--------|---------|
| `400` | Missing `service` or `title`, a field over its limit, invalid device data, or invalid JSON |
| `401` | Invalid or missing `X-Service-Key` / `X-API-Key` |
| `404` | Notification not found |
| `405` | Wrong HTTP method for the endpoint |
| `413` | Request body too large |
| `503` | Push notifications are not configured (`POST /api/devices` only) |
