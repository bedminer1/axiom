#!/usr/bin/env bash
set -Eeuo pipefail

server="${HOUND_SERVER:-root@178.105.168.55}"
remote_dir="${HOUND_REMOTE_DIR:-/opt/hound}"
site_dir="${HOUND_SITE_DIR:-/var/www/hound}"

cd "$(dirname "$0")"
npm --prefix website ci
npm --prefix website run build
cargo build --locked --release

ssh -o BatchMode=yes "$server" "mkdir -p '$remote_dir/pfi-filter/.pfi-cache' '$site_dir'"
rsync -az --delete --delay-updates website/build/ "$server:$site_dir/"
rsync -az --delete --exclude '.git/' --exclude '.pfi-cache/' vendor/pfi-filter/ "$server:$remote_dir/pfi-filter/"
rsync -az target/release/hound "$server:$remote_dir/hound"
scp -q deploy/hound.service "$server:/etc/systemd/system/hound.service"

ssh -o BatchMode=yes "$server" "
  id hound >/dev/null 2>&1 || useradd --system --home-dir '$remote_dir' --shell /usr/sbin/nologin hound
  chown hound:hound '$remote_dir/pfi-filter/.pfi-cache'
  test -x '$remote_dir/.venv/bin/python' || python3 -m venv '$remote_dir/.venv'
  '$remote_dir/.venv/bin/python' -m pip install --quiet -e '$remote_dir/pfi-filter' rapidfuzz
  systemctl daemon-reload
  systemctl enable --now hound
  systemctl restart hound
  curl -fsS http://127.0.0.1:5050/api/health
"

echo "Hound files deployed. Caddy serves $site_dir and proxies /api/* to 127.0.0.1:5050."
