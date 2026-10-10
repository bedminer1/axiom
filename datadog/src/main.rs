use anyhow::{Context, Result};
use axiom_datadog::{
    deepseek::DeepSeek, default_runtime_path, detector::Detector, project_dir, project_path,
    store::Store, worker,
};
use fs2::FileExt;
use std::{
    env,
    fs::OpenOptions,
    sync::{Arc, Mutex},
};
use teloxide::prelude::*;
use teloxide::types::{CallbackQuery, ChatMemberKind};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("axiom_datadog=info,teloxide=off,teloxide_core=off,reqwest=off,sqlx=off")
        .init();
    let env_file = project_dir().join(".env");
    if env_file.exists() {
        dotenvy::from_path(env_file).context("cannot read datadog/.env")?;
    }
    let token = env::var("TELOXIDE_TOKEN").context("set TELOXIDE_TOKEN in datadog/.env")?;
    let bot = Bot::new(token);
    bot.get_me()
        .await
        .map_err(|_| anyhow::anyhow!("Telegram authentication/network check failed"))?;
    let model = DeepSeek::new(
        env::var("DEEPSEEK_API_KEY").context("set DEEPSEEK_API_KEY in datadog/.env")?,
        env::var("DEEPSEEK_BASE_URL").unwrap_or_else(|_| "https://api.deepseek.com".into()),
        env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-flash".into()),
    )?;
    let store = Store::open(&project_path(
        env::var("BOT_DATA_DIR").unwrap_or_else(|_| "data".into()),
    ))
    .await?;
    let lock_path = store.data.join("datadog.lock");
    let instance_lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    instance_lock.try_lock_exclusive().with_context(|| {
        format!(
            "another DataDog process is already running (lock: {})",
            lock_path.display()
        )
    })?;
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
    let yolo_enabled = env::var("YOLO_ANNOTATIONS")
        .ok()
        .is_some_and(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes"));
    let detector = if yolo_enabled {
        match tokio::task::spawn_blocking(move || {
            Detector::load(&yolo_path, &runtime, confidence, iou)
        })
        .await?
        {
            Ok(d) => Some(Arc::new(Mutex::new(d))),
            Err(_) => {
                tracing::warn!("YOLO unavailable; evidence collection remains enabled");
                None
            }
        }
    } else {
        tracing::info!("YOLO annotations disabled; VLM observations remain enabled");
        None
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
        tokio::spawn(worker::loading_loop(bot.clone(), store.clone())),
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
        )
        .branch(Update::filter_callback_query().endpoint(callback));
    tracing::info!("DataDog ready: local command routing with DeepSeek visual analysis");
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
    instance_lock.unlock()?;
    Ok(())
}
fn number_setting(key: &str, default: f32) -> Result<f32> {
    match env::var(key) {
        Ok(v) => v.parse().with_context(|| format!("invalid {key}")),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}
async fn ingest(bot: Bot, msg: Message, store: Store) -> Result<()> {
    let bot_added = if let Some(members) = msg.new_chat_members() {
        let bot_id = bot.get_me().await.ok().map(|me| me.id);
        bot_id.is_some_and(|id| members.iter().any(|member| member.id == id))
    } else {
        false
    };
    let topic = msg.thread_id.map(|v| v.0.0 as i64).unwrap_or(0);
    let owner = if msg.chat.is_private() {
        msg.from.as_ref().map(|user| user.id.0 as i64)
    } else if let Ok(Some(owner)) = store.chat_owner(msg.chat.id.0, topic).await {
        Some(owner)
    } else {
        bot.get_chat_administrators(msg.chat.id)
            .await
            .ok()
            .and_then(|members| {
                members
                    .into_iter()
                    .find(|member| matches!(member.kind, ChatMemberKind::Owner(_)))
                    .map(|member| member.user.id.0 as i64)
            })
    };
    if store.ingest(&msg, owner, bot_added).await.is_err() {
        tracing::error!(
            chat = msg.chat.id.0,
            message = msg.id.0,
            "Could not persist Telegram message"
        );
    }
    Ok(())
}
async fn callback(bot: Bot, query: CallbackQuery, store: Store) -> Result<()> {
    let Some(data) = query.data.as_deref() else {
        return Ok(());
    };
    let mut parts = data.split(':');
    let kind = parts.next();
    if kind == Some("project") {
        let confirm = match parts.next() {
            Some("confirm") => true,
            Some("cancel") => false,
            _ => return Ok(()),
        };
        let Some(message) = query.regular_message() else {
            return Ok(());
        };
        let callback_id = query.id.clone();
        let message_chat = message.chat.id;
        let message_id = message.id;
        let topic = message
            .thread_id
            .map(|thread| thread.0.0 as i64)
            .unwrap_or(0);
        let result = store
            .resolve_project_confirmation(query.from.id.0 as i64, message.chat.id.0, topic, confirm)
            .await;
        let text = result.unwrap_or_else(|error| {
            error
                .to_string()
                .lines()
                .next()
                .unwrap_or("Could not update project")
                .to_owned()
        });
        bot.answer_callback_query(callback_id)
            .text(&text)
            .show_alert(false)
            .await?;
        let _ = bot.edit_message_text(message_chat, message_id, text).await;
        return Ok(());
    }
    if kind == Some("legal") {
        if parts.next() != Some("accept") {
            return Ok(());
        }
        let actor = query.from.id.0 as i64;
        let mode = parts.next();
        let target = match mode {
            Some("self" | "group") => actor,
            Some(value) => match value.parse::<i64>() {
                Ok(value) => value,
                Err(_) => return Ok(()),
            },
            None => return Ok(()),
        };
        let chat = query.message.as_ref().map(|m| m.chat().id.0);
        let topic = query
            .regular_message()
            .and_then(|message| message.thread_id)
            .map(|thread| thread.0.0 as i64)
            .unwrap_or(0);
        let display_name = match query.from.last_name.as_deref() {
            Some(last) => format!("{} {last}", query.from.first_name),
            None => query.from.first_name.clone(),
        };
        let result = store
            .accept_legal(
                actor,
                target,
                query.from.username.as_deref(),
                &display_name,
                chat,
                topic,
                mode != Some("group"),
            )
            .await;
        let text = result.unwrap_or_else(|error| {
            error
                .to_string()
                .lines()
                .next()
                .unwrap_or("Could not record acceptance")
                .to_owned()
        });
        bot.answer_callback_query(query.id)
            .text(text)
            .show_alert(false)
            .await?;
        return Ok(());
    }
    Ok(())
}
