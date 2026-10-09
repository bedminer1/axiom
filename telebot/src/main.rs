use anyhow::{Context, Result};
use axiom_telebot::{
    deepseek::DeepSeek, default_runtime_path, detector::Detector, project_dir, project_path,
    store::Store, worker,
};
use std::{
    env,
    sync::{Arc, Mutex},
};
use teloxide::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("axiom_telebot=info,teloxide=off,teloxide_core=off,reqwest=off,sqlx=off")
        .init();
    let env_file = project_dir().join(".env");
    if env_file.exists() {
        dotenvy::from_path(env_file).context("cannot read telebot/.env")?;
    }
    let token = env::var("TELOXIDE_TOKEN").context("set TELOXIDE_TOKEN in telebot/.env")?;
    let bot = Bot::new(token);
    bot.get_me()
        .await
        .map_err(|_| anyhow::anyhow!("Telegram authentication/network check failed"))?;
    let model = DeepSeek::new(
        env::var("DEEPSEEK_API_KEY").context("set DEEPSEEK_API_KEY in telebot/.env")?,
        env::var("DEEPSEEK_BASE_URL").unwrap_or_else(|_| "https://api.deepseek.com".into()),
        env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-flash".into()),
    )?;
    let store = Store::open(&project_path(
        env::var("BOT_DATA_DIR").unwrap_or_else(|_| "data".into()),
    ))
    .await?;
    let yolo_path =
        project_path(env::var("YOLO_MODEL").unwrap_or_else(|_| "models/crack.onnx".into()));
    let runtime = env::var("ORT_DYLIB_PATH")
        .map(project_path)
        .unwrap_or_else(|_| default_runtime_path());
    let confidence = number_setting("YOLO_CONFIDENCE", 0.35)?;
    let iou = number_setting("YOLO_IOU", 0.45)?;
    let model_hash = if let Ok(bytes) = tokio::fs::read(&yolo_path).await {
        use sha2::{Digest, Sha256};
        format!("crack-yolov8s:sha256:{:x}", Sha256::digest(bytes))
    } else {
        "unavailable".into()
    };
    let detector = match tokio::task::spawn_blocking(move || {
        Detector::load(&yolo_path, &runtime, confidence, iou)
    })
    .await?
    {
        Ok(d) => Some(Arc::new(Mutex::new(d))),
        Err(_) => {
            tracing::warn!("YOLO unavailable; evidence collection remains enabled");
            None
        }
    };
    let tasks = [
        tokio::spawn(worker::archive_loop(bot.clone(), store.clone())),
        tokio::spawn(worker::collect_loop(bot.clone(), store.clone(), model)),
        tokio::spawn(worker::yolo_loop(
            bot.clone(),
            store.clone(),
            detector,
            model_hash,
        )),
        tokio::spawn(worker::send_loop(bot.clone(), store.clone())),
    ];
    // teloxide shared_state.rs: inject durable application state into message endpoints.
    let handler = dptree::entry()
        .branch(
            Update::filter_message()
                .filter(|msg: Message| worker::relevant(&msg))
                .endpoint(ingest),
        )
        .branch(
            Update::filter_channel_post()
                .filter(|msg: Message| worker::relevant(&msg))
                .endpoint(ingest),
        );
    tracing::info!(
        "DataDog ready: conversational projects, evidence checklist, DeepSeek high reasoning"
    );
    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![store])
        .default_handler(|_| async {})
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
    for task in tasks {
        task.abort();
    }
    Ok(())
}
fn number_setting(key: &str, default: f32) -> Result<f32> {
    match env::var(key) {
        Ok(v) => v.parse().with_context(|| format!("invalid {key}")),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}
async fn ingest(msg: Message, store: Store) -> Result<()> {
    if store.ingest(&msg).await.is_err() {
        tracing::error!(
            chat = msg.chat.id.0,
            message = msg.id.0,
            "Could not persist Telegram message"
        );
    }
    Ok(())
}
