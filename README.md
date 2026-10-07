# Axiom

A minimal concrete-crack screening app. It selects one random image from `lipi17/building-cracks`, asks Meta SAM 3 to segment `crack in concrete`, and calculates uncalibrated geometry metrics in Rust.

## Architecture

- Rust/Axum: random sample selection, worker orchestration, geometry, provisional screening grade, static file serving.
- Python: official SAM 3 PyTorch inference only.
- SvelteKit: small monochrome inspection interface derived from the portfolio design language.

## Requirements

The official SAM 3 repository currently requires Python 3.12+, PyTorch 2.7+, CUDA 12.6+, an approved request for the gated checkpoint, and an authenticated Hugging Face account. SAM inference therefore needs a CUDA machine; Apple Silicon can still run and develop the web/API portions.

## Setup

```bash
./scripts/setup-dataset.sh
npm --prefix web install
npm --prefix web run build
uv sync --project ml
hf auth login
cargo run
```

Open `http://127.0.0.1:3000`.

For a dedicated CUDA environment, point the server at any command with the same CLI contract:

```bash
AXIOM_SAM_COMMAND="/path/to/python -m axiom_ml.segment" cargo run
```

The command receives `--image PATH --output PATH`, writes a binary PNG mask, and prints JSON containing `mask_path`, `score`, `instances`, and `model`.

For frontend iteration, run `./scripts/dev.sh` and open Vite's printed URL.

## Metrics and grading

Coverage is the mask fraction of the 640×640 image. Dominant orientation is the principal axis of foreground pixels. Length is a centerline proxy and mean width is mask area divided by that proxy. The provisional minor/moderate/severe grade uses visible coverage thresholds only. All measurements remain pixels because this dataset has no physical scale; the result is explicitly not an engineering assessment.

## Telegram photo bot

The independent Rust/teloxide bot in [`telebot/`](telebot/README.md) accepts
photos, detects crack candidates with YOLOv8s, and returns annotated images.
See its README for model setup, token configuration and running the bot.

## Checks

```bash
cargo fmt --check
cargo test
npm --prefix web run check
npm --prefix web run build
```
