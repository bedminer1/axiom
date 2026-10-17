# Hound

A small PFI inspection draft: create a project, add one photo per elevation in each inspection section, and review a short AI observation for each photo.

## Run

From `hound/`:

```sh
cargo run
```

The Rust API reads `DEEPSEEK_API_KEY`, `DEEPSEEK_BASE_URL`, and `DEEPSEEK_MODEL` from the environment or, by default, `../archived/datadog/.env`. Set `DEEPSEEK_ENV_FILE` to use another env file. Use a DeepSeek model that accepts images, such as `deepseek-flash`.

In another terminal:

```sh
cd website
npm install
npm run dev
```

Photos are saved in this browser. Each selected photo is sent to DeepSeek for its observation. The inspector can edit the observation and mark it reviewed. Progress counts reviewed photo slots only; it does not measure inspection coverage.

## Policies

[`legal/PRIVACY.md`](legal/PRIVACY.md) · [`legal/TERMS.md`](legal/TERMS.md)
