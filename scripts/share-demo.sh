#!/usr/bin/env bash
# Exposes the already-running Axiom server through Tailscale across networks.
set -euo pipefail

mode="${1:-private}"

if ! command -v tailscale >/dev/null 2>&1; then
  echo "Tailscale is required: https://tailscale.com/download" >&2
  exit 1
fi

if ! curl --fail --silent http://127.0.0.1:3000/api/health >/dev/null; then
  echo "Start Axiom first with: cargo run" >&2
  exit 1
fi

if [[ "$mode" == "public" ]]; then
  tailscale funnel --bg --yes 3000
  tailscale funnel status
elif [[ "$mode" == "private" ]]; then
  tailscale serve --bg --yes 3000
  tailscale serve status
else
  echo "Usage: ./scripts/share-demo.sh [private|public]" >&2
  exit 1
fi
