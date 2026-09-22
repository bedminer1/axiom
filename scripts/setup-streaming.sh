#!/usr/bin/env bash
# Downloads the pinned standalone MediaMTX binary used by the RTMP launcher.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="1.21.1"
ARCHIVE="mediamtx_v${VERSION}_linux_amd64.tar.gz"
URL="https://github.com/bluenviron/mediamtx/releases/download/v${VERSION}/${ARCHIVE}"
BIN_DIR="$ROOT/streaming/bin"
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT

mkdir -p "$BIN_DIR"
echo "Downloading MediaMTX v${VERSION}…"
curl -fL "$URL" -o "$TEMP_DIR/$ARCHIVE"
tar -xzf "$TEMP_DIR/$ARCHIVE" -C "$TEMP_DIR"
install -m 0755 "$TEMP_DIR/mediamtx" "$BIN_DIR/mediamtx"
echo "Installed $BIN_DIR/mediamtx"
