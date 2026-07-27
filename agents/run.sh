#!/usr/bin/env bash
# Cron-friendly wrapper: loads .env (so HOME/keys/PATH are set for headless
# claude) then runs the agent. Usage: ./run.sh <agent-name> [--dry-run]
set -euo pipefail
cd "$(dirname "$0")"
set -a
[ -f .env ] && . ./.env
set +a
mkdir -p logs
exec python3 runner.py "$@"
