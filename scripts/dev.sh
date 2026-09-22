#!/usr/bin/env bash
set -euo pipefail
trap 'kill 0' EXIT
cargo run &
npm --prefix web run dev &
wait

