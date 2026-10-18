# Hound

PFI inspection draft. Add an overview to each visual elevation and as many close-range photos as needed. The [pfi-filter](https://github.com/CricketGaming21/pfi-filter) CLI suggests observations and returns photo assessments. DeepSeek drafts provisional repair recommendations for visible defects. If the filter is unavailable, Hound uses its short DeepSeek photo prompt.

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

Photos, the optional floor plan, and edited observations are saved in this browser. Add a floor plan in Section 01 if available. Otherwise Section 03 shows an orientation sketch using the Section 02 elevation photos. Photo positions are guesses until checked; the sketch cannot establish building geometry or inspection coverage. Progress counts reviewed photos, not inspected façade area.

Six illustrative JPEGs and their proposed demo sides are in [`website/static/demo/`](website/static/demo/README.md). Add `north-overview.jpg` to the North visual slot and the detail photos to the close-range section. These photos are not verified as one building.

## Hetzner deployment

`./deploy-hetzner.sh` builds the static site and Rust API locally, syncs them to Hetzner, installs the filter dependencies, and restarts `hound.service`. Caddy serves the site at `https://datadog.properrobotics.org/` and proxies `/api/*` to the local API. The site is password protected; the local credential is stored outside Git at `~/.config/hound/hetzner-access.txt` (username `hound`). The service reads the existing server-side DeepSeek settings through a systemd credential. The old Datadog bot container is separate from this website deployment.

## Policies

[`legal/PRIVACY.md`](legal/PRIVACY.md) · [`legal/TERMS.md`](legal/TERMS.md)
