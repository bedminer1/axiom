#!/usr/bin/env bash
set -Eeuo pipefail

server="${DATADOG_SERVER:-root@178.105.168.55}"
remote_dir="${DATADOG_REMOTE_DIR:-/opt/axiom-datadog}"
env_file="${DATADOG_LOCAL_ENV:-.env}"

[[ -f "$env_file" ]] || { echo "Missing environment file: $env_file" >&2; exit 1; }
[[ -f models/crack.onnx && -f models/libonnxruntime.so ]] || {
  echo "Missing ONNX model/runtime files under models/" >&2
  exit 1
}

echo "Checking $server..."
if ! ssh -o BatchMode=yes "$server" \
  "command -v docker >/dev/null && docker compose version >/dev/null"; then
  echo "Docker or the Compose plugin is unavailable on $server." >&2
  exit 1
fi
ssh "$server" "mkdir -p '$remote_dir'"

echo "Uploading application files..."
rsync -az --info=progress2 \
  --exclude '.env' \
  --exclude '.venv/' \
  --exclude 'data/' \
  --exclude 'run/' \
  --exclude 'target/' \
  --exclude 'models/crack.pt' \
  ./ "$server:$remote_dir/"

env_tmp="/tmp/axiom-datadog.env.$$"
echo "Installing the protected environment file..."
scp -q "$env_file" "$server:$env_tmp"
ssh "$server" "install -o root -g root -m 600 '$env_tmp' /etc/axiom-datadog.env && rm -f '$env_tmp'"

echo "Building and starting DataDog..."
ssh "$server" "cd '$remote_dir' && \
  docker compose -f compose.production.yml build && \
  docker compose -f compose.production.yml up -d --remove-orphans && \
  docker compose -f compose.production.yml ps && \
  docker compose -f compose.production.yml logs --tail=40 datadog"

echo "Deployment finished. Telegram long polling needs no inbound port or reverse proxy."
