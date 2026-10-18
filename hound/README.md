# Hound

PFI inspection draft. Add an overview to each visual elevation and close-range photos for each exterior type. The [pfi-filter](https://github.com/CricketGaming21/pfi-filter) CLI suggests observations and returns photo assessments. SoCLaas `x-test-2` drafts provisional repair recommendations and supplies the photo fallback.

## One-time setup

```sh
git submodule update --init
cd hound
npm --prefix website install
uv venv .venv --python 3.12
uv pip install --python .venv/bin/python -e vendor/pfi-filter rapidfuzz
```

`rapidfuzz` is imported by the current filter release but is not listed in its package dependencies. Locally, the API reads `SOCLAAS_API_KEY` from the environment or `~/.hermes/.env`; `SOCLAAS_MODEL` defaults to `x-test-2`. Set `SOCLAAS_ENV_FILE` to use another credential file. The key stays in the Rust API and is passed to the filter as its OpenAI-compatible vision setting.

## Run

```sh
cd hound
cargo run
```

This starts the Rust API and the Vite website at `http://127.0.0.1:5173/`. Stop both with Ctrl-C. `PFI_FILTER_OFFLINE=1 cargo run` makes the filter use its cache; uncached photos then use the SoCLaas fallback. Set `PFI_FILTER_DIR` or `PFI_FILTER_PYTHON` to override the default submodule and Python environment paths.

The current pfi-filter CLI does not run its optional YOLO detector: `--yolo` is informational. The `models.zip` ONNX file is installed locally at `models/crack.onnx` with its hash in `models/source.json`, ready for that future filter step. The large ONNX file is ignored by Git.

Photos, the optional floor plan, and edited observations are saved in this browser. Add a floor plan in Section 01 if available. Otherwise Section 03 shows an orientation sketch using the Section 02 elevation photos. Photo positions are guesses until checked; the sketch cannot establish building geometry or inspection coverage. Coverage is entered from measured façade and inspected areas in Section 03.

Six labelled illustrative JPEGs and a click-by-click walkthrough are in [`website/static/demo/`](website/static/demo/README.md). Start with the North visual overview and one rendered-wall crack close-up. The photos are not verified as one building.

## Hetzner deployment

`./deploy-hetzner.sh` builds the static site and Rust API locally, syncs them to Hetzner, restarts `hound.service`, and updates the Caddy site block. Caddy serves `https://datadog.properrobotics.org/` without a shared login and proxies `/api/*` to the local API. Before the first deployment, place `SOCLAAS_API_KEY=...` in `/etc/hound-soclaas.env` with root-only permissions; systemd passes it to Hound as a credential. The old Datadog bot container is separate from this website deployment.

## Policies

[`legal/PRIVACY.md`](legal/PRIVACY.md) · [`legal/TERMS.md`](legal/TERMS.md)
