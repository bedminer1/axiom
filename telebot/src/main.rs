use std::{
    env,
    sync::{Arc, Mutex},
    time::Instant,
};

use anyhow::{Context, Result, ensure};
use axiom_telebot::{default_runtime_path, detector::Detector, project_dir, project_path};
use teloxide::{
    net::Download,
    prelude::*,
    types::{ChatAction, FileId, InputFile, ReplyParameters},
};
use tokio::sync::Semaphore;

const MAX_DOWNLOAD: u32 = 20 * 1024 * 1024;
const OUTPUT_PREFIX: &str = "DataDog · ";

#[derive(Clone)]
struct AppState {
    detector: Arc<Mutex<Detector>>,
    capacity: Arc<Semaphore>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Keep request URLs and the bot token out of third-party HTTP/polling logs.
    tracing_subscriber::fmt()
        .with_env_filter("axiom_telebot=info,teloxide=off,teloxide_core=off")
        .init();
    let env_file = project_dir().join(".env");
    if env_file.exists() {
        dotenvy::from_path(env_file).context("cannot read telebot/.env")?;
    }
    let token = env::var("TELOXIDE_TOKEN").context("set TELOXIDE_TOKEN in telebot/.env")?;
    ensure!(
        !token.trim().is_empty() && token != "your_botfather_token",
        "TELOXIDE_TOKEN is missing"
    );
    let bot = Bot::new(token);
    let model = project_path(env::var("YOLO_MODEL").unwrap_or_else(|_| "models/crack.onnx".into()));
    let runtime = env::var("ORT_DYLIB_PATH")
        .map(project_path)
        .unwrap_or_else(|_| default_runtime_path());
    let confidence = number_setting("YOLO_CONFIDENCE", 0.35)?;
    let iou = number_setting("YOLO_IOU", 0.45)?;
    let detector =
        tokio::task::spawn_blocking(move || Detector::load(&model, &runtime, confidence, iou))
            .await??;
    // Startup verifies the token, but never prints it or includes a request URL.
    let me = bot
        .get_me()
        .await
        .map_err(|_| anyhow::anyhow!("Telegram authentication/network check failed"))?;
    tracing::info!(
        username = me.username(),
        "Crack detector ready; send a photo"
    );
    let state = AppState {
        detector: Arc::new(Mutex::new(detector)),
        capacity: Arc::new(Semaphore::new(8)),
    };
    // Mirrors teloxide's dispatching_features.rs and shared_state.rs examples:
    // filter updates, inject shared state, handle the endpoint, shut down on Ctrl-C.
    // There are deliberately no command handlers or command registration calls.
    let handler = dptree::entry()
        .branch(
            Update::filter_message()
                .filter(is_image)
                .endpoint(handle_image),
        )
        .branch(
            Update::filter_channel_post()
                .filter(is_image)
                .endpoint(handle_image),
        );
    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![state])
        .default_handler(|_| async {})
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
    Ok(())
}

fn number_setting(key: &str, default: f32) -> Result<f32> {
    match env::var(key) {
        Ok(value) => value
            .parse()
            .with_context(|| format!("{key} must be a number")),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error).with_context(|| format!("cannot read {key}")),
    }
}

fn is_image(msg: Message) -> bool {
    // Ignore bot-authored images; the caption marker also prevents a channel
    // posting its returned image from feeding it back through the pipeline.
    if msg.from.as_ref().is_some_and(|user| user.is_bot)
        || msg
            .caption()
            .is_some_and(|caption| caption.starts_with(OUTPUT_PREFIX))
    {
        return false;
    }
    image_file(&msg).or_else(|| document_file(&msg)).is_some()
}

fn image_file(msg: &Message) -> Option<(FileId, u32)> {
    if let Some(photo) = msg
        .photo()?
        .iter()
        .max_by_key(|photo| u64::from(photo.width) * u64::from(photo.height))
    {
        return Some((photo.file.id.clone(), photo.file.size));
    }
    None
}

// Telegram also allows original-resolution photos to be sent as documents.
fn document_file(msg: &Message) -> Option<(FileId, u32)> {
    let document = msg.document()?;
    let mime = document.mime_type.as_ref().map(|mime| mime.as_ref());
    let supported_mime = matches!(mime, Some("image/jpeg" | "image/png" | "image/webp"));
    let supported_extension = document.file_name.as_ref().is_some_and(|name| {
        name.rsplit('.').next().is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "webp"
            )
        })
    });
    (supported_mime || supported_extension).then(|| (document.file.id.clone(), document.file.size))
}

async fn handle_image(bot: Bot, msg: Message, state: AppState) -> Result<()> {
    if let Err(error) = process_image(&bot, &msg, state).await {
        let safe_error = error.to_string().replace(bot.token(), "[redacted]");
        tracing::warn!(
            chat_id = msg.chat.id.0,
            message_id = msg.id.0,
            error = safe_error,
            "Photo processing failed"
        );
        let mut reply = bot.send_message(msg.chat.id,
            "I couldn't process this image. Please try again with a JPEG, PNG or WebP under 20 MB.")
            .reply_parameters(ReplyParameters::new(msg.id).allow_sending_without_reply());
        if let Some(thread) = msg.thread_id {
            reply = reply.message_thread_id(thread);
        }
        if reply.await.is_err() {
            tracing::warn!("Could not send photo-processing error reply");
        }
    }
    Ok(())
}

async fn process_image(bot: &Bot, msg: &Message, state: AppState) -> Result<()> {
    let (file_id, size) = image_file(msg)
        .or_else(|| document_file(msg))
        .context("no supported image")?;
    ensure!(size <= MAX_DOWNLOAD, "file exceeds download limit");
    let _permit = state
        .capacity
        .try_acquire_owned()
        .context("image queue is full")?;
    let start = Instant::now();
    let mut action = bot.send_chat_action(msg.chat.id, ChatAction::UploadPhoto);
    if let Some(thread) = msg.thread_id {
        action = action.message_thread_id(thread);
    }
    let _ = action.await;
    let file = bot.get_file(file_id).await?;
    ensure!(file.size <= MAX_DOWNLOAD, "file exceeds download limit");
    let mut bytes = Vec::with_capacity(file.size as usize);
    bot.download_file(&file.path, &mut bytes).await?;
    ensure!(
        bytes.len() <= MAX_DOWNLOAD as usize,
        "file exceeds download limit"
    );
    let detector = state.detector.clone();
    let processed = tokio::task::spawn_blocking(move || {
        detector
            .lock()
            .map_err(|_| anyhow::anyhow!("detector lock poisoned"))?
            .process(&bytes)
    })
    .await??;
    let caption = if processed.detections == 0 {
        format!("{OUTPUT_PREFIX}No cracks detected at the current confidence threshold.")
    } else {
        format!(
            "{OUTPUT_PREFIX}{} crack candidate{}. Bounding boxes show model detections.",
            processed.detections,
            if processed.detections == 1 { "" } else { "s" }
        )
    };
    let mut reply = bot
        .send_photo(
            msg.chat.id,
            InputFile::memory(processed.jpeg).file_name("cracks.jpg"),
        )
        .caption(caption)
        .reply_parameters(ReplyParameters::new(msg.id).allow_sending_without_reply());
    if let Some(thread) = msg.thread_id {
        reply = reply.message_thread_id(thread);
    }
    reply.await?;
    tracing::info!(
        detections = processed.detections,
        passes = processed.passes,
        elapsed_ms = start.elapsed().as_millis(),
        "Annotated image returned"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn message(body: Value) -> Message {
        let mut value = json!({
            "message_id": 12, "date": 1700000000,
            "chat": {"id": 42, "type": "private", "first_name": "Inspector"},
            "from": {"id": 42, "is_bot": false, "first_name": "Inspector"}
        });
        for (key, field) in body.as_object().unwrap() {
            value[key] = field.clone();
        }
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn uses_largest_photo_and_accepts_original_image_documents() {
        let photo = message(json!({"photo": [
            {"file_id": "thumb", "file_unique_id": "a", "width": 90, "height": 90, "file_size": 100},
            {"file_id": "large", "file_unique_id": "b", "width": 1280, "height": 960, "file_size": 50000}
        ]}));
        assert!(is_image(photo.clone()));
        assert_eq!(image_file(&photo).unwrap().0.0, "large");
        let document = message(json!({"document": {
            "file_id": "original", "file_unique_id": "c", "file_name": "inspection.JPG",
            "mime_type": "image/jpeg", "file_size": 100000
        }}));
        assert!(is_image(document.clone()));
        assert_eq!(document_file(&document).unwrap().0.0, "original");
    }

    #[test]
    fn ignores_commands_unrelated_files_and_own_channel_output() {
        assert!(!is_image(message(json!({"text": "/start"}))));
        assert!(!is_image(message(json!({"document": {
            "file_id": "pdf", "file_unique_id": "d", "mime_type": "application/pdf", "file_name": "plan.pdf"
        }}))));
        assert!(!is_image(message(json!({
            "caption": format!("{OUTPUT_PREFIX}1 crack candidate."),
            "photo": [{"file_id": "reply", "file_unique_id": "e", "width": 640, "height": 640}]
        }))));
    }
}
