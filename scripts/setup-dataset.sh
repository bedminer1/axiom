#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
archive="$repo_root/data/train.zip"
destination="$repo_root/data/dataset"
mkdir -p "$destination"
curl -L --fail --progress-bar "https://huggingface.co/datasets/lipi17/building-cracks/resolve/main/data/train.zip?download=true" -o "$archive"
unzip -q -o "$archive" -d "$destination"
rm "$archive"
printf 'Dataset ready at %s\n' "$destination"

