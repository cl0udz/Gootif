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
| `metadata` | object | No | `{}` | Arbitrary key-value pairs for extra context |

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

## Error Responses

| Status | Meaning |
|--------|---------|
| `400` | Missing `service` or `title`, or invalid JSON |
| `401` | Invalid or missing `X-Service-Key` |
| `405` | Wrong HTTP method (must be POST) |
