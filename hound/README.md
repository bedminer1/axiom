# Hound

PFI inspection draft. Add a photo to a visual or close-range slot; the [pfi-filter](https://github.com/CricketGaming21/pfi-filter) CLI suggests an observation and returns its photo assessment. If the filter is unavailable or has no observation, Hound uses its short DeepSeek photo prompt.

## One-time setup

```sh
git submodule update --init
cd hound
npm --prefix website install
uv venv .venv --python 3.12
uv pip install --python .venv/bin/python -e vendor/pfi-filter rapidfuzz
```

`rapidfuzz` is imported by the current filter release but is not listed in its package dependencies. The API reads the DeepSeek settings from the environment or `../archived/datadog/.env` and passes them to the filter as its OpenAI-compatible vision settings. The key stays on the server.

## Run

```sh
cd hound
cargo run
```

This starts the Rust API and the Vite website at `http://127.0.0.1:5173/`. Stop both with Ctrl-C. `PFI_FILTER_OFFLINE=1 cargo run` makes the filter use its cache; uncached photos then use the DeepSeek fallback. Set `PFI_FILTER_DIR` or `PFI_FILTER_PYTHON` to override the default submodule and Python environment paths.

The current pfi-filter CLI does not run its optional YOLO detector: `--yolo` is informational. The `models.zip` ONNX file is installed locally at `models/crack.onnx` with its hash in `models/source.json`, ready for that future filter step. The large ONNX file is ignored by Git.

Photos and edited observations are saved in this browser. The uploaded photo is sent to DeepSeek through the local Rust API for filter analysis. Progress counts reviewed photo slots only, not inspection coverage.

## Policies

[`legal/PRIVACY.md`](legal/PRIVACY.md) · [`legal/TERMS.md`](legal/TERMS.md)
