# DataDog photo bot

A separate Rust/teloxide bot: receive an image, detect crack candidates with
crack-trained YOLOv8s, draw bounding boxes and confidence labels, and reply with
the annotated JPEG. There are no Telegram commands. Text and unrelated files
are ignored. Works independently of the web/SAM app.

## Setup and run

From the repository root:

```sh
uv run --python 3.12 telebot/setup_model.py
cp telebot/.env.example telebot/.env
chmod 600 telebot/.env
# Put your BotFather token in telebot/.env.
cargo run --release --manifest-path telebot/Cargo.toml
```

If `telebot/.env` is already configured, keep that file. The setup script is
idempotent and verifies the pinned checkpoint's SHA-256 before loading it.
Python/Ultralytics are used only to export the model once and obtain the native
ONNX Runtime library. The running bot, inference, image processing and Telegram
handling are Rust. CPU inference works on Apple Silicon; no CUDA or SAM needed.

Open the bot's Telegram conversation and send a photo. You can also send a JPEG,
PNG or WebP **as a file** to preserve its resolution, up to Telegram's 20 MB bot
download limit. Each image in an album is processed and returned individually.
Replies stay in the originating chat and forum topic and reference the photo.

For a group, make the bot an admin or disable privacy mode through BotFather's
`/setprivacy` and re-add it. Telegram otherwise withholds ordinary group photos.
For a channel, add the bot with permission to post photos. Existing history is
not imported. No commands need to be configured in BotFather.

Only one polling instance should use the token. Stop the foreground instance
with Ctrl-C. If a webhook is already configured for this token, remove it before
running a polling instance.

## Detection

- Uses the single-class `crack` checkpoint from
  [Utbird/crack_detection](https://huggingface.co/Utbird/crack_detection), pinned
  to revision `a5382b48f32fab49f2881b18bc3125592c318c64`.
- A 640-pixel whole-image pass plus overlapping 640-pixel crops keeps more
  detail in distant views. Boxes are mapped back to the image and deduplicated
  using non-maximum suppression.
- Images larger than 3072 pixels on their longest side are resized to bound CPU
  work. Phone EXIF orientation is applied before detecting and drawing.
- `YOLO_CONFIDENCE` defaults to `0.35`; lower it to admit weaker detections, at
  the expense of more false positives. `YOLO_IOU` defaults to `0.45`.
- An image with no detections is still returned, with an explicit caption.
- Detection confidence is a model score, not an engineering severity rating.
  Far-away cracks that are not resolved in the source image can be missed;
  representative façade photographs are needed to evaluate this checkpoint.

Images are processed in memory, with a serialized reusable model and a bounded
queue. Incoming images are not archived. The `.env`, downloaded weights, native
runtime and `run/` logs are ignored by Git. Keep the token out of source files.

## Checks

```sh
cargo fmt --manifest-path telebot/Cargo.toml --check
cargo test --manifest-path telebot/Cargo.toml
cargo clippy --manifest-path telebot/Cargo.toml --all-targets -- -D warnings
cargo run --release --manifest-path telebot/Cargo.toml --example detect -- input.jpg output.jpg
```

The offline example uses the same Rust detector as Telegram. It does not send
messages. Tests cover letterbox coordinates, crop coverage, overlapping-box
suppression, photo resolution selection and message filtering.

## References

The dispatcher and dependency injection follow teloxide's official examples:

- [dispatching_features.rs](https://github.com/teloxide/teloxide/blob/master/crates/teloxide/examples/dispatching_features.rs)
- [shared_state.rs](https://github.com/teloxide/teloxide/blob/master/crates/teloxide/examples/shared_state.rs)
- [Download API](https://docs.rs/teloxide/0.17.0/teloxide/net/trait.Download.html)
- [Ultralytics ONNX export](https://docs.ultralytics.com/modes/export/)
