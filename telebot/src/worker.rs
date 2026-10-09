use crate::{
    deepseek::{Action, DeepSeek, Label, Proposal},
    detector::Detector,
    store::{Evidence, Job, Outgoing, Store, now, parse_id},
};
use anyhow::{Context, Result, ensure};
use image::{ImageDecoder, ImageReader, codecs::jpeg::JpegEncoder};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::Cursor,
    sync::{Arc, Mutex},
    time::Duration,
};
use teloxide::{
    net::Download,
    prelude::*,
    types::{ChatId, InputFile, MessageId, ReplyParameters, ThreadId},
};

pub const OUTPUT_PREFIX: &str = "DataDog · ";
pub fn image_file(msg: &Message) -> Option<(String, String, u32, Option<String>)> {
    if let Some(photo) = msg.photo().and_then(|photos| {
        photos
            .iter()
            .max_by_key(|p| u64::from(p.width) * u64::from(p.height))
    }) {
        return Some((
            photo.file.id.0.clone(),
            photo.file.unique_id.0.clone(),
            photo.file.size,
            None,
        ));
    }
    let doc = msg.document()?;
    let mime = doc.mime_type.as_ref().map(|m| m.as_ref());
    let extension = doc
        .file_name
        .as_ref()
        .and_then(|n| n.rsplit('.').next())
        .map(str::to_ascii_lowercase);
    if matches!(mime, Some("image/jpeg" | "image/png" | "image/webp"))
        || matches!(extension.as_deref(), Some("jpg" | "jpeg" | "png" | "webp"))
    {
        Some((
            doc.file.id.0.clone(),
            doc.file.unique_id.0.clone(),
            doc.file.size,
            doc.file_name.clone(),
        ))
    } else {
        None
    }
}
pub fn relevant(msg: &Message) -> bool {
    if msg.from.as_ref().is_some_and(|u| u.is_bot) {
        return false;
    }
    let text = msg.text().or_else(|| msg.caption()).unwrap_or("");
    if text.starts_with(OUTPUT_PREFIX) || text.trim_start().starts_with('/') {
        return false;
    }
    image_file(msg).is_some() || msg.text().is_some()
}
// Downloads have their own durable worker: inference latency never blocks archiving.
pub async fn archive_loop(bot: Bot, store: Store) {
    loop {
        let job=sqlx::query_as::<_,Job>("SELECT id,chat,topic,telegram_id,sender,text,raw,project,attempts,analysis FROM messages WHERE status='receiving' AND next_attempt<=? ORDER BY id LIMIT 1").bind(now()).fetch_optional(&store.pool).await;
        match job {
            Ok(Some(job)) => {
                let result:Result<i64>=async {
                    let msg:Message=serde_json::from_str(&job.raw)?;
                    let id=store.reserve_evidence(&job).await?;
                    if store.evidence(id).await?.is_some_and(|e|e.preview_path.is_none()) {
                        save_photo(&bot,&store,id,image_file(&msg).context("missing photo")?).await?;
                    }
                    sqlx::query("UPDATE messages SET status='pending',attempts=0,next_attempt=0,error=NULL WHERE id=?").bind(job.id).execute(&store.pool).await?;
                    Ok(id)
                }.await;
                match result {
                    Ok(id) => {
                        let _=store.enqueue(&job,"reply",&format!("Saved E{id:04}. Processing its checklist label and crack annotation."),None,Some(id)).await;
                    }
                    Err(_) => {
                        let attempts = job.attempts + 1;
                        let _=sqlx::query("UPDATE messages SET attempts=?,next_attempt=?,status=?,error='Image download or decoding failed' WHERE id=?").bind(attempts).bind(now()+30*attempts).bind(if attempts>=3{"failed"}else{"receiving"}).bind(job.id).execute(&store.pool).await;
                        if attempts >= 3 {
                            let _=store.enqueue(&job,"reply","I saved the message but couldn’t prepare this image. Please resend a JPEG, PNG or WebP under 20 MB, preferably as a file.",None,None).await;
                        }
                        tracing::warn!(message = job.id, "Image archive deferred");
                    }
                }
            }
            _ => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
}
pub async fn collect_loop(bot: Bot, store: Store, model: DeepSeek) {
    loop {
        match store.next_job().await {
            Ok(Some(job)) => {
                if let Err(error) = collect_one(&bot, &store, &model, &job).await {
                    // Error strings here are generated locally; HTTP bodies and tokens are never logged.
                    tracing::warn!(message = job.id, "Evidence processing deferred");
                    let safe = format!(
                        "Evidence processing failed: {}",
                        error
                            .to_string()
                            .split('\n')
                            .next()
                            .unwrap_or("unknown error")
                    );
                    if store.retry(&job, &safe).await.is_err() {
                        tracing::error!("Could not persist retry");
                    }
                }
            }
            Ok(None) => tokio::time::sleep(Duration::from_millis(500)).await,
            Err(_) => {
                tracing::error!("Could not read durable queue");
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    }
}
async fn collect_one(bot: &Bot, store: &Store, model: &DeepSeek, job: &Job) -> Result<()> {
    let msg: Message = serde_json::from_str(&job.raw)?;
    if let Some(file) = image_file(&msg) {
        let id = store.reserve_evidence(job).await?;
        if store
            .evidence(id)
            .await?
            .context("missing reserved evidence")?
            .path
            .is_none()
        {
            save_photo(bot, store, id, file).await?;
        }
        // A manual correction can complete an earlier failed classification job.
        if store
            .evidence(id)
            .await?
            .is_some_and(|e| e.label.is_some() && e.label.as_deref() != Some("uncertain"))
            && job.analysis.is_none()
        {
            sqlx::query("UPDATE messages SET status='done' WHERE id=?")
                .bind(job.id)
                .execute(&store.pool)
                .await?;
            return Ok(());
        }
    }
    let context = store.context(job).await?;
    let proposal = if let Some(analysis) = &job.analysis {
        serde_json::from_str::<Proposal>(analysis)?
    } else {
        let evidence = store.current_evidence(job).await?;
        let preview = if let Some(path) = evidence.and_then(|e| e.preview_path) {
            Some(tokio::fs::read(path).await?)
        } else {
            None
        };
        match model
            .analyze(context.clone(), &job.text, preview.as_deref())
            .await
        {
            Ok(analysis) => {
                store.save_analysis(job, &analysis).await?;
                analysis.proposal
            }
            Err(error) => {
                // A small exact-label parser keeps explicit manual labels usable during outages.
                if let Some(p) = manual_label(&job.text, &context) {
                    let analysis = crate::deepseek::Analysis {
                        proposal: p.clone(),
                        model: "manual-label".into(),
                        usage: Value::Null,
                    };
                    store.save_analysis(job, &analysis).await?;
                    p
                } else {
                    return Err(error);
                }
            }
        }
    };
    match store.apply(job, &proposal, &context).await {
        Ok(()) => Ok(()),
        Err(_) => {
            let mut clarification = proposal;
            clarification.action = Action::Clarify;
            clarification.elevations.clear();
            clarification.elevation = None;
            clarification.finding = None;
            clarification.evidence = None;
            clarification.label = None;
            clarification.description = None;
            clarification.question=Some("I couldn’t apply that assignment. Please specify the location, elevation, finding ID and photo label.".into());
            store.apply(job, &clarification, &context).await
        }
    }
}
async fn save_photo(
    bot: &Bot,
    store: &Store,
    id: i64,
    file: (String, String, u32, Option<String>),
) -> Result<()> {
    let (file_id, unique, size, filename) = file;
    ensure!(
        size <= 20 * 1024 * 1024,
        "image exceeds Telegram download limit"
    );
    let telegram = bot
        .get_file(teloxide::types::FileId(file_id.clone()))
        .await
        .map_err(|_| anyhow::anyhow!("Telegram file lookup failed"))?;
    let mut bytes = Vec::new();
    bot.download_file(&telegram.path, &mut bytes)
        .await
        .map_err(|_| anyhow::anyhow!("Telegram image download failed"))?;
    ensure!(bytes.len() <= 20 * 1024 * 1024, "download exceeds limit");
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let size = bytes.len() as i64;
    let dir = store.data.join("evidence").join(format!("E{id:04}"));
    tokio::fs::create_dir_all(&dir).await?;
    // Keep even an undecodable source file; preview preparation can be retried.
    let raw_path = dir.join("source.bin");
    let temp = dir.join("source.tmp");
    tokio::fs::write(&temp, &bytes).await?;
    tokio::fs::rename(&temp, &raw_path).await?;
    sqlx::query("UPDATE evidence SET path=?,sha256=?,file_id=?,file_unique_id=?,filename=?,bytes=? WHERE id=?")
        .bind(raw_path.to_string_lossy().as_ref()).bind(&hash).bind(&file_id).bind(&unique).bind(&filename).bind(size).bind(id).execute(&store.pool).await?;
    let (preview, width, height, mime, extension, exif) =
        tokio::task::spawn_blocking(move || decode(&bytes)).await??;
    let path = dir.join(format!("source.{extension}"));
    tokio::fs::rename(&raw_path, &path).await?;
    let preview_path = dir.join("preview.jpg");
    tokio::fs::write(&preview_path, preview).await?;
    sqlx::query("UPDATE evidence SET path=?,preview_path=?,sha256=?,file_id=?,file_unique_id=?,filename=?,mime=?,bytes=?,width=?,height=?,exif=?,yolo_status='pending' WHERE id=?")
  .bind(path.to_string_lossy().as_ref()).bind(preview_path.to_string_lossy().as_ref()).bind(hash).bind(file_id).bind(unique).bind(filename).bind(mime).bind(size).bind(width as i64).bind(height as i64).bind(exif.to_string()).bind(id).execute(&store.pool).await?;
    Ok(())
}
fn decode(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32, &'static str, &'static str, Value)> {
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let (mime, extension) = match reader.format() {
        Some(image::ImageFormat::Jpeg) => ("image/jpeg", "jpg"),
        Some(image::ImageFormat::Png) => ("image/png", "png"),
        Some(image::ImageFormat::WebP) => ("image/webp", "webp"),
        _ => anyhow::bail!("unsupported image format"),
    };
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut image = image::DynamicImage::from_decoder(decoder)?;
    let (width, height) = (image.width(), image.height());
    image.apply_orientation(orientation);
    let mut metadata = serde_json::Map::new();
    if let Ok(exif) = exif::Reader::new().read_from_container(&mut Cursor::new(bytes)) {
        for f in exif.fields() {
            metadata.insert(
                format!("{}:{}", f.ifd_num, f.tag),
                json!(f.display_value().with_unit(&exif).to_string()),
            );
        }
    }
    let image = image.thumbnail(1600, 1600).to_rgb8();
    let mut preview = Vec::new();
    JpegEncoder::new_with_quality(&mut preview, 88).encode_image(&image)?;
    Ok((
        preview,
        width,
        height,
        mime,
        extension,
        Value::Object(metadata),
    ))
}
fn manual_label(text: &str, context: &Value) -> Option<Proposal> {
    let text = text.trim();
    let lower = text.to_lowercase();
    let label = match lower.as_str() {
        "building overview" | "building overview photo" => Label::BuildingOverview,
        "plan" | "plan drawing" => Label::Plan,
        _ => {
            if lower.ends_with("elevation overview") {
                Label::ElevationOverview
            } else if lower.ends_with("close-up") || lower.ends_with("close up") {
                Label::CloseUp
            } else if lower.ends_with("context photo") {
                Label::ContextPhoto
            } else {
                return None;
            }
        }
    };
    let evidence = context["reply_evidence"]["id"]
        .as_str()
        .or_else(|| context["pending_evidence"]["id"].as_str())
        .or_else(|| context["current_evidence"]["id"].as_str())?
        .to_owned();
    let elevation = if label == Label::ElevationOverview {
        Some(
            text[..text.len() - "elevation overview".len()]
                .trim()
                .to_owned(),
        )
    } else {
        None
    };
    let finding = text
        .split_whitespace()
        .next()
        .filter(|s| parse_id(s, 'F').is_some())
        .map(str::to_owned);
    Some(Proposal {
        action: Action::UpdateEvidence,
        location: None,
        new_location: None,
        elevations: vec![],
        elevation,
        level: None,
        finding,
        new_finding: false,
        evidence: Some(evidence),
        label: Some(label),
        description: None,
        question: None,
        certain: true,
    })
}
pub async fn yolo_loop(
    bot: Bot,
    store: Store,
    detector: Option<Arc<Mutex<Detector>>>,
    model_id: String,
) {
    loop {
        let row=sqlx::query_as::<_,Evidence>("SELECT id,message,project,elevation,finding,label,description,path,preview_path,yolo_attempts FROM evidence WHERE yolo_status='pending' AND yolo_next_attempt<=? ORDER BY id LIMIT 1").bind(now()).fetch_optional(&store.pool).await;
        match row {
            Ok(Some(e)) => {
                if let Err(_error) = annotate(&bot, &store, detector.clone(), &model_id, &e).await {
                    let attempts = e.yolo_attempts + 1;
                    let _=sqlx::query("UPDATE evidence SET yolo_attempts=?,yolo_status=?,yolo_next_attempt=? WHERE id=?").bind(attempts).bind(if attempts>=3{"failed"}else{"pending"}).bind(now()+60*attempts).bind(e.id).execute(&store.pool).await;
                    if attempts >= 3 {
                        if let Ok(job)=sqlx::query_as::<_,Job>("SELECT id,chat,topic,telegram_id,sender,text,raw,project,attempts,analysis FROM messages WHERE id=?").bind(e.message).fetch_one(&store.pool).await{let _=store.enqueue(&job,"reply","Photo saved, but crack annotation is unavailable. Checklist collection can continue.",None,Some(e.id)).await;}
                    }
                    tracing::warn!(evidence = e.id, "Crack annotation deferred");
                }
            }
            _ => tokio::time::sleep(Duration::from_secs(1)).await,
        }
    }
}
async fn annotate(
    _bot: &Bot,
    store: &Store,
    detector: Option<Arc<Mutex<Detector>>>,
    model_id: &str,
    e: &Evidence,
) -> Result<()> {
    let detector = detector.context("YOLO is unavailable")?;
    let path = e.path.as_ref().context("image is not downloaded")?;
    let bytes = tokio::fs::read(path).await?;
    let processed = tokio::task::spawn_blocking(move || {
        detector
            .lock()
            .map_err(|_| anyhow::anyhow!("detector lock failed"))?
            .process(&bytes)
    })
    .await??;
    let output = std::path::Path::new(path).with_file_name("annotated.jpg");
    tokio::fs::write(&output, &processed.jpeg).await?;
    let data = json!({"model":model_id,"confidence_threshold":std::env::var("YOLO_CONFIDENCE").unwrap_or_else(|_|"0.35".into()),"iou_threshold":std::env::var("YOLO_IOU").unwrap_or_else(|_|"0.45".into()),"boxes":processed.boxes,"coordinate_space":"annotated_image_pixels","width":processed.width,"height":processed.height,"passes":processed.passes});
    let mut tx = store.pool.begin().await?;
    sqlx::query("UPDATE evidence SET annotated_path=?,yolo=?,yolo_status='done' WHERE id=?")
        .bind(output.to_string_lossy().as_ref())
        .bind(data.to_string())
        .bind(e.id)
        .execute(&mut *tx)
        .await?;
    let caption = format!(
        "E{:04} · {} crack candidate(s). Bounding boxes show model detections.",
        e.id, processed.detections
    );
    sqlx::query("INSERT INTO outbox(chat,topic,reply_to,kind,text,path,evidence) SELECT chat,topic,telegram_id,'photo',?,?,? FROM messages WHERE id=?").bind(caption).bind(output.to_string_lossy().as_ref()).bind(e.id).bind(e.message).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn send_loop(bot: Bot, store: Store) {
    loop {
        match store.next_outgoing().await {
            Ok(Some(out)) => {
                match send_one(&bot, &store, &out).await {
                    Ok(id) => {
                        let _=sqlx::query("UPDATE outbox SET delivered_at=?,telegram_message=?,error=NULL WHERE id=?").bind(now()).bind(id).bind(out.id).execute(&store.pool).await;
                    }
                    Err(_) => {
                        let attempts = out.attempts + 1;
                        let _=sqlx::query("UPDATE outbox SET attempts=?,next_attempt=?,error='Telegram delivery failed' WHERE id=?").bind(attempts).bind(now()+15*2i64.pow(attempts.min(8) as u32)).bind(out.id).execute(&store.pool).await;
                        tracing::warn!(outbox = out.id, "Telegram delivery deferred");
                    }
                }
            }
            _ => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
}
async fn send_one(bot: &Bot, store: &Store, out: &Outgoing) -> Result<i32> {
    let chat = ChatId(out.chat);
    let topic = if out.topic != 0 {
        Some(ThreadId(MessageId(out.topic as i32)))
    } else {
        None
    };
    let prefix = |text: &str| {
        if text.starts_with(OUTPUT_PREFIX) {
            text.to_owned()
        } else {
            format!("{OUTPUT_PREFIX}{text}")
        }
    };
    if out.kind == "photo" {
        let mut request = bot
            .send_photo(
                chat,
                InputFile::file(out.path.as_ref().context("missing outgoing photo")?),
            )
            .caption(prefix(&out.text));
        if let Some(topic) = topic {
            request = request.message_thread_id(topic);
        }
        if let Some(id) = out.reply_to {
            request = request.reply_parameters(
                ReplyParameters::new(MessageId(id as i32)).allow_sending_without_reply(),
            );
        }
        return Ok(request.await?.id.0);
    }
    let mut text = prefix(&out.text);
    if out.kind == "checklist" {
        // Coalesce album/checklist updates; the displayed content always comes from the database.
        let newer:Option<i64>=sqlx::query_scalar("SELECT id FROM outbox WHERE chat=? AND topic=? AND kind='checklist' AND id>? AND delivered_at IS NULL LIMIT 1").bind(out.chat).bind(out.topic).bind(out.id).fetch_optional(&store.pool).await?;
        if newer.is_some() {
            return Ok(0);
        }
        let row = sqlx::query_as::<_, (Option<String>, Option<i64>)>(
            "SELECT project,checklist_message FROM chats WHERE chat=? AND topic=?",
        )
        .bind(out.chat)
        .bind(out.topic)
        .fetch_optional(&store.pool)
        .await?;
        let (project, existing) = row.unwrap_or_default();
        text = store.checklist(project.as_deref()).await?;
        if text.chars().count() <= 4000 {
            if let Some(id) = existing {
                match bot
                    .edit_message_text(chat, MessageId(id as i32), &text)
                    .await
                {
                    Ok(_) => return Ok(id as i32),
                    Err(error) if error.to_string().contains("message is not modified") => {
                        return Ok(id as i32);
                    }
                    Err(_) => {}
                }
            }
        }
    }
    let parts = split_text(&text, 3900);
    let mut first = 0;
    for part in parts {
        let mut request = bot.send_message(chat, prefix(&part));
        if let Some(topic) = topic {
            request = request.message_thread_id(topic);
        }
        if let Some(id) = out.reply_to {
            request = request.reply_parameters(
                ReplyParameters::new(MessageId(id as i32)).allow_sending_without_reply(),
            );
        }
        let msg = request.await?;
        if first == 0 {
            first = msg.id.0;
        }
    }
    if out.kind == "checklist" && text.chars().count() <= 4000 {
        sqlx::query("UPDATE chats SET checklist_message=? WHERE chat=? AND topic=?")
            .bind(first)
            .bind(out.chat)
            .bind(out.topic)
            .execute(&store.pool)
            .await?;
    }
    Ok(first)
}
fn split_text(text: &str, max: usize) -> Vec<String> {
    let mut parts = vec![];
    let mut current = String::new();
    for c in text.chars() {
        if current.chars().count() >= max {
            parts.push(current);
            current = String::new();
        }
        current.push(c);
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}
