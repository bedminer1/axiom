#!/usr/bin/env bash
# Starts RTMP ingestion and converts the drone feed to low-latency browser HLS.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LIVE_DIR="$ROOT/data/live"
INPUT_URL="${AXIOM_RTMP_INPUT:-rtmp://127.0.0.1:1935/live/drone}"

mkdir -p "$LIVE_DIR"
if [[ ! -x "$ROOT/streaming/bin/mediamtx" ]]; then
  echo "MediaMTX is missing. Run ./scripts/setup-streaming.sh once." >&2
  exit 1
fi

"$ROOT/streaming/bin/mediamtx" "$ROOT/streaming/mediamtx.yml" &
MEDIAMTX_PID=$!

clear_playlist() {
  rm -f "$LIVE_DIR/drone.m3u8" "$LIVE_DIR"/drone-*.ts
}

cleanup() {
  clear_playlist
  if [[ -n "${MEDIAMTX_PID:-}" ]]; then kill "$MEDIAMTX_PID" 2>/dev/null || true; fi
}
trap cleanup EXIT INT TERM

DEMO_IP="$(tailscale ip -4 2>/dev/null || hostname -I | awk '{print $1}')"
echo "RTMP receiver ready at rtmp://${DEMO_IP}:1935/live/drone"
echo "Waiting for a publisher; leave this terminal running."

# Retry forever so the feed recovers after disconnects and battery changes.
while true; do
  clear_playlist
  ffmpeg -hide_banner -loglevel warning \
    -flags low_delay -i "$INPUT_URL" \
    -map 0:v:0 -an -c:v libx264 -preset veryfast -tune zerolatency \
    -pix_fmt yuv420p -g 30 -keyint_min 30 -sc_threshold 0 \
    -f hls -hls_time 1 -hls_list_size 4 \
    -hls_flags delete_segments+append_list+omit_endlist+independent_segments \
    -hls_segment_filename "$LIVE_DIR/drone-%06d.ts" \
    "$LIVE_DIR/drone.m3u8" || true
  echo "Stream disconnected; retrying in 1 second…"
  sleep 1
done
