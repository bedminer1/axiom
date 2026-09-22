# Axiom

A minimal concrete-crack screening app. It selects one random image from `lipi17/building-cracks`, generates a binary crack mask, and calculates uncalibrated geometry metrics in Rust. By default it uses Samir Mohamed's concrete-crack U-Net; Meta SAM 3 remains available as an optional backend.

## Architecture

- Rust/Axum: random sample selection, worker orchestration, geometry, provisional screening grade, static file serving.
- Python: concrete-crack U-Net inference by default, with optional official SAM 3 inference.
- SvelteKit: small monochrome inspection interface derived from the portfolio design language.

## Requirements

The default U-Net checkpoint is public and downloads automatically from Hugging Face on first use. It supports either CUDA or CPU inference. The optional official SAM 3 backend requires Python 3.12+, PyTorch 2.7+, CUDA, an approved request for the gated checkpoint, and an authenticated Hugging Face account.

## Setup

```bash
./scripts/setup-dataset.sh
npm --prefix web install
npm --prefix web run build
uv sync --project ml
cargo run
```

Open `http://127.0.0.1:3000`.

Choose the U-Net or SAM 3 from the model selector in the running interface. The server no longer needs a model launch argument. SAM 3 requires approved checkpoint access and a completed `hf auth login`.

The U-Net preserves thin cracks with overlapping 256×256 native-resolution tiles. Its mask uses hysteresis thresholding: pixels above `0.35` seed a crack, connected pixels down to `0.10` extend it, and components smaller than 128 pixels are discarded. These values were selected by visually checking representative images, but the model still misses some irregular concrete cracks because its training domain differs. They can be overridden through the worker command, for example:

```bash
AXIOM_UNET_COMMAND="uv run --project ml python -m axiom_ml.crack_segment --low-threshold 0.2 --high-threshold 0.45 --min-component-pixels 64" cargo run
```

The source dataset was stretched to 640×640 and does not provide camera distance, intrinsics, or a known-size reference. For demonstration only, Axiom applies an assumed `0.12 mm/px` conversion and clearly marks the resulting millimetre values and severity labels as simulated. They are not field measurements.

For a dedicated environment, point either backend at any command with the same CLI contract:

```bash
AXIOM_UNET_COMMAND="/path/to/python -m axiom_ml.crack_segment" cargo run
AXIOM_SAM_COMMAND="/path/to/python -m axiom_ml.segment" cargo run
```

The command receives `--image PATH --output PATH`, writes a binary PNG mask, and prints JSON containing `mask_path`, `score`, `instances`, and `model`.

For frontend iteration, run `./scripts/dev.sh` and open Vite's printed URL.

## Metrics and grading

Coverage is the mask fraction of the 640×640 image. Dominant orientation is the principal axis of foreground pixels. Length is a centerline proxy and mean width is mask area divided by that proxy. Demo severity uses simulated mean-width bands: under 0.3 mm is hairline, under 1.0 mm is moderate, and 1.0 mm or more is wide. These bands and physical values are presentation data, not an engineering assessment.

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
