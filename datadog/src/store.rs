use crate::{
    database::{self as db, Database, DatabaseConfig, DbRow, FromDbRow, Transaction},
    deepseek::{Action, Label, Proposal},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{io::AsyncWriteExt, process::Command};

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
pub fn normalize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
const TERMS_VERSION: &str = "2026-10-10";
const PRIVACY_VERSION: &str = "2026-10-10";

pub fn title_case_name(value: &str) -> String {
    value
        .split_whitespace()
        .enumerate()
        .map(|(index, word)| {
            let lower = word.to_lowercase();
            if index > 0 && matches!(lower.as_str(), "and" | "of" | "the" | "at" | "in") {
                return lower;
            }
            let mut chars = lower.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn requested_project_name(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();
    [
        "start a new project",
        "create a new project",
        "start new project",
        "create project",
        "new project",
    ]
    .iter()
    .find_map(|prefix| {
        lower.strip_prefix(prefix).and_then(|remainder| {
            let offset = trimmed.len() - remainder.len();
            let value = trimmed[offset..]
                .trim_start_matches([' ', ',', ':', '-'])
                .trim();
            (!value.is_empty()).then(|| value.to_owned())
        })
    })
}

fn looks_like_project_name(text: &str) -> bool {
    let words = normalize(text)
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    !text.contains('?')
        && !words.is_empty()
        && words.len() <= 8
        && !words.iter().any(|word| {
            matches!(
                word.as_str(),
                "i" | "you"
                    | "we"
                    | "he"
                    | "she"
                    | "they"
                    | "it"
                    | "my"
                    | "your"
                    | "our"
                    | "show"
                    | "what"
                    | "why"
                    | "how"
                    | "please"
                    | "help"
                    | "send"
                    | "generate"
                    | "add"
                    | "remove"
                    | "rename"
                    | "stop"
                    | "pause"
                    | "resume"
            )
        })
}

fn actionable_group_message(
    msg: &teloxide::types::Message,
    text: &str,
    awaiting_name: bool,
) -> bool {
    if awaiting_name || crate::worker::image_file(msg).is_some() {
        return true;
    }
    if msg
        .reply_to_message()
        .is_some_and(|reply| crate::worker::image_file(reply).is_some())
    {
        return true;
    }
    let text = normalize(text);
    [
        "project",
        "inspection",
        "checklist",
        "report",
        "elevation",
        "finding",
        "evidence",
        "photo",
        "overview",
        "close up",
        "plan",
        "facade",
        "defect",
        "crack",
        "spalling",
        "corrosion",
        "level",
        "zone",
        "team",
        "archive",
        "restore",
        "rename",
    ]
    .iter()
    .any(|term| text.contains(term))
}

fn welcome_text(role: &str, can_create: bool) -> String {
    let access = if role == "superuser" {
        "Access: superuser"
    } else if role == "user" {
        "Access: project creator"
    } else {
        "Access: team member"
    };
    let next = if can_create {
        "Send the building name to create a project.\nExample: Star Vista"
    } else {
        "Open a project group and send a photo or say: show checklist\nThe group owner creates the project."
    };
    format!(
        "DataDog is ready.\n{access}\n\n{next}\n\nBugs or feature requests? Message @dkffjlrtm on Telegram."
    )
}

fn group_hello_text() -> String {
    format!(
        "Hello, I’m DataDog. I’ll listen for photos and notes in this group, organise the evidence, and keep the checklist updated.\n\n{}\n\nEach person should tap OK once. Then send the building name to create this group’s project.\n\nBugs or feature requests? Message @dkffjlrtm on Telegram.",
        legal_text()
    )
}

fn help_text() -> &'static str {
    "What DataDog can do\n\nTalk naturally — no commands needed.\n\n• Add DataDog to an inspection group and start a project.\n• Send photos with captions. DataDog organises the evidence, updates the checklist and flags visible conditions for review.\n• Ask to see the checklist, team or current project.\n• Ask for a draft report when the inspection is ready.\n\nHaving an unrelated group conversation? Say “stop listening”. Say “start listening” when you want DataDog to handle photos and inspection notes again.\n\nBugs or feature requests: @dkffjlrtm"
}

fn legal_text() -> &'static str {
    "Before you start\n\n• We store the account and project data needed to run DataDog. We do not sell your data.\n• AI output can be wrong. Inspectors remain responsible for findings and final reports.\n• Do not attack the service, bypass access controls, or use malicious prompts.\n\nTap OK to accept the Privacy Policy and Terms of Use."
}
#[derive(Clone)]
pub struct Store {
    pub db: Database,
    pub data: PathBuf,
}
#[derive(Clone)]
pub struct Job {
    pub id: i64,
    pub chat: i64,
    pub topic: i64,
    pub telegram_id: i64,
    pub sender: String,
    pub text: String,
    pub raw: String,
    pub project: Option<String>,
    pub inspection_run: Option<String>,
    pub attempts: i64,
    pub analysis: Option<String>,
}
#[derive(Clone)]
pub struct Evidence {
    pub id: i64,
    pub message: i64,
    pub project: Option<String>,
    pub inspection_run: Option<String>,
    pub elevation: Option<i64>,
    pub finding: Option<i64>,
    pub label: Option<String>,
    pub description: Option<String>,
    pub path: Option<String>,
    pub preview_path: Option<String>,
    pub yolo_attempts: i64,
}
pub struct Outgoing {
    pub id: i64,
    pub chat: i64,
    pub topic: i64,
    pub reply_to: Option<i64>,
    pub kind: String,
    pub text: String,
    pub path: Option<String>,
    pub evidence: Option<i64>,
    pub attempts: i64,
    pub source_message: Option<i64>,
}

impl FromDbRow for Job {
    fn from_db_row(row: DbRow) -> Result<Self> {
        Ok(Self {
            id: row.try_get("id")?,
            chat: row.try_get("chat")?,
            topic: row.try_get("topic")?,
            telegram_id: row.try_get("telegram_id")?,
            sender: row.try_get("sender")?,
            text: row.try_get("text")?,
            raw: row.try_get("raw")?,
            project: row.try_get("project")?,
            inspection_run: row.try_get("inspection_run")?,
            attempts: row.try_get("attempts")?,
            analysis: row.try_get("analysis")?,
        })
    }
}

impl FromDbRow for Evidence {
    fn from_db_row(row: DbRow) -> Result<Self> {
        Ok(Self {
            id: row.try_get("id")?,
            message: row.try_get("message")?,
            project: row.try_get("project")?,
            inspection_run: row.try_get("inspection_run")?,
            elevation: row.try_get("elevation")?,
            finding: row.try_get("finding")?,
            label: row.try_get("label")?,
            description: row.try_get("description")?,
            path: row.try_get("path")?,
            preview_path: row.try_get("preview_path")?,
            yolo_attempts: row.try_get("yolo_attempts")?,
        })
    }
}

impl FromDbRow for Outgoing {
    fn from_db_row(row: DbRow) -> Result<Self> {
        Ok(Self {
            id: row.try_get("id")?,
            chat: row.try_get("chat")?,
            topic: row.try_get("topic")?,
            reply_to: row.try_get("reply_to")?,
            kind: row.try_get("kind")?,
            text: row.try_get("text")?,
            path: row.try_get("path")?,
            evidence: row.try_get("evidence")?,
            attempts: row.try_get("attempts")?,
            source_message: row.try_get("source_message")?,
        })
    }
}
impl Store {
    pub async fn open(data: &Path) -> Result<Self> {
        tokio::fs::create_dir_all(data.join("evidence")).await?;
        let db = Database::connect(DatabaseConfig::from_env(data)?).await?;
        tracing::info!(backend = db.backend_name(), "Persistent database ready");
        Ok(Self {
            db,
            data: data.to_owned(),
        })
    }
    pub async fn chat_owner(&self, chat: i64, topic: i64) -> Result<Option<i64>> {
        Ok(db::query_scalar::<Option<i64>>(
            "SELECT owner_telegram_id FROM chats WHERE chat=? AND topic=?",
        )
        .bind(chat)
        .bind(topic)
        .fetch_optional(&self.db)
        .await?
        .flatten())
    }
    pub async fn ingest(
        &self,
        msg: &teloxide::types::Message,
        owner_telegram_id: Option<i64>,
        bot_added: bool,
    ) -> Result<()> {
        let Some(user) = &msg.from else {
            return Ok(());
        };
        let raw = serde_json::to_string(msg)?;
        let topic = msg.thread_id.map(|v| v.0.0 as i64).unwrap_or(0);
        let user_id = user.id.0 as i64;
        let sender = format!("user:{user_id}");
        let text = msg.text().or_else(|| msg.caption()).unwrap_or("");
        let is_start = text.split_whitespace().next() == Some("/start")
            || text
                .split_whitespace()
                .next()
                .is_some_and(|v| v.starts_with("/start@"));
        let username = user.username.as_deref().map(str::to_lowercase);
        let display_name = match user.last_name.as_deref() {
            Some(last) => format!("{} {last}", user.first_name),
            None => user.first_name.clone(),
        };
        let mut tx = self.db.begin().await?;
        db::query("INSERT INTO chats(chat,topic,owner_telegram_id,is_private) VALUES(?,?,?,?) ON CONFLICT(chat,topic) DO UPDATE SET owner_telegram_id=COALESCE(excluded.owner_telegram_id,chats.owner_telegram_id),is_private=excluded.is_private")
            .bind(msg.chat.id.0).bind(topic).bind(owner_telegram_id).bind(msg.chat.is_private()).execute(&mut tx).await?;
        let access = db::query("SELECT id,role,status,terms_version,privacy_version FROM users WHERE telegram_user_id=?")
            .bind(user_id)
            .fetch_optional(&mut tx)
            .await?;
        if access.is_none() {
            if let Some(handle) = username.as_deref() {
                db::query("UPDATE users SET username=NULL WHERE lower(username)=? AND telegram_user_id<>?")
                    .bind(handle).bind(user_id).execute(&mut tx).await?;
            }
            let result = db::query("INSERT INTO users(telegram_user_id,username,display_name,role,status,private_chat_id,requested_at,created_at,updated_at) VALUES(?,?,?,'guest','pending',?,?,?,?)")
                .bind(user_id).bind(&username).bind(&display_name).bind(msg.chat.id.0)
                .bind(now()).bind(now()).bind(now()).execute(&mut tx).await?;
            let applicant_id = result.last_insert_rowid();
            db::query("INSERT INTO user_access_audit(target_user,action,created_at) VALUES(?,'legal_prompted',?)")
                .bind(applicant_id).bind(now()).execute(&mut tx).await?;
            db::query("INSERT INTO outbox(chat,topic,reply_to,kind,text,path) VALUES(?,?,?,'legal_consent',?,?)")
                .bind(msg.chat.id.0).bind(topic).bind(msg.id.0)
                .bind(legal_text()).bind(user_id.to_string())
                .execute(&mut tx).await?;
            tx.commit().await?;
            return Ok(());
        }
        let access = access.unwrap();
        let mut role: String = access.get("role");
        let status: String = access.get("status");
        if let Some(handle) = username.as_deref() {
            db::query(
                "UPDATE users SET username=NULL WHERE lower(username)=? AND telegram_user_id<>?",
            )
            .bind(handle)
            .bind(user_id)
            .execute(&mut tx)
            .await?;
        }
        db::query("UPDATE users SET username=?,display_name=?,private_chat_id=CASE WHEN ? THEN ? ELSE private_chat_id END,updated_at=? WHERE id=?")
            .bind(&username).bind(&display_name).bind(msg.chat.is_private()).bind(msg.chat.id.0).bind(now()).bind(access.get::<i64,_>("id"))
            .execute(&mut tx).await?;
        if status == "rejected" {
            db::query("INSERT INTO outbox(chat,topic,reply_to,kind,text) VALUES(?,?,?,'reply',?)")
                .bind(msg.chat.id.0)
                .bind(topic)
                .bind(msg.id.0)
                .bind("Your DataDog access is unavailable. Contact the service owner if this looks wrong.")
                .execute(&mut tx)
                .await?;
            tx.commit().await?;
            return Ok(());
        }
        let accepted = access.get::<Option<String>, _>("terms_version").as_deref()
            == Some(TERMS_VERSION)
            && access
                .get::<Option<String>, _>("privacy_version")
                .as_deref()
                == Some(PRIVACY_VERSION);
        db::query("INSERT INTO chat_participants(chat,topic,user,first_seen_at,last_seen_at) VALUES(?,?,?,?,?) ON CONFLICT(chat,topic,user) DO UPDATE SET last_seen_at=excluded.last_seen_at")
            .bind(msg.chat.id.0).bind(topic).bind(access.get::<i64,_>("id")).bind(now()).bind(now()).execute(&mut tx).await?;
        if !accepted {
            db::query("INSERT INTO outbox(chat,topic,reply_to,kind,text,path) VALUES(?,?,?,'legal_consent',?,?)")
                .bind(msg.chat.id.0).bind(topic).bind(msg.id.0).bind(legal_text()).bind(user_id.to_string()).execute(&mut tx).await?;
            tx.commit().await?;
            return Ok(());
        }
        if bot_added && !msg.chat.is_private() {
            let welcomed: i64 =
                db::query_scalar("SELECT welcomed FROM chats WHERE chat=? AND topic=?")
                    .bind(msg.chat.id.0)
                    .bind(topic)
                    .fetch_one(&mut tx)
                    .await?;
            if welcomed == 0 {
                db::query("UPDATE chats SET welcomed=1,awaiting_project_name=CASE WHEN project IS NULL THEN 1 ELSE 0 END WHERE chat=? AND topic=?")
                    .bind(msg.chat.id.0).bind(topic).execute(&mut tx).await?;
                db::query(
                    "INSERT INTO outbox(chat,topic,kind,text) VALUES(?,?,'group_legal_consent',?)",
                )
                .bind(msg.chat.id.0)
                .bind(topic)
                .bind(group_hello_text())
                .execute(&mut tx)
                .await?;
            }
            tx.commit().await?;
            return Ok(());
        }
        if role == "guest" {
            db::query("UPDATE users SET role='user',updated_at=? WHERE id=?")
                .bind(now())
                .bind(access.get::<i64, _>("id"))
                .execute(&mut tx)
                .await?;
            role = "user".into();
        }
        let command = text
            .split_whitespace()
            .next()
            .unwrap_or("")
            .split('@')
            .next()
            .unwrap_or("");
        let normalized_role_question = text.trim().to_lowercase();
        let normalized_role_question = normalized_role_question
            .trim_end_matches(['?', '.', '!'])
            .trim();
        let role_question = matches!(
            normalized_role_question,
            "am i a superuser" | "am i an admin" | "what is my role" | "who am i"
        );
        let help_question = matches!(
            normalized_role_question,
            "help"
                | "how do i use datadog"
                | "how do i use the bot"
                | "how to use datadog"
                | "what can datadog do"
                | "what can you do"
        );
        if command == "/help" || help_question {
            db::query("INSERT INTO outbox(chat,topic,reply_to,kind,text) VALUES(?,?,?,'reply',?)")
                .bind(msg.chat.id.0)
                .bind(topic)
                .bind(msg.id.0)
                .bind(help_text())
                .execute(&mut tx)
                .await?;
            tx.commit().await?;
            return Ok(());
        }
        let greeting = matches!(
            normalized_role_question,
            "hi" | "hello" | "hey" | "hiya" | "good morning" | "good afternoon" | "good evening"
        );
        if msg.chat.is_private() && greeting {
            db::query("INSERT INTO outbox(chat,topic,reply_to,kind,text) VALUES(?,?,?,'reply',?)")
                .bind(msg.chat.id.0)
                .bind(topic)
                .bind(msg.id.0)
                .bind(format!("Hi!\n\n{}", help_text()))
                .execute(&mut tx)
                .await?;
            tx.commit().await?;
            return Ok(());
        }
        if command == "/me" || role_question {
            let reply = match role.as_str() {
                "superuser" => {
                    format!("{display_name} · superuser · all projects and user management")
                }
                "user" => format!("{display_name} · project creator · assigned projects"),
                _ => format!("{display_name} · team member · assigned projects"),
            };
            db::query("INSERT INTO outbox(chat,topic,reply_to,kind,text) VALUES(?,?,?,'reply',?)")
                .bind(msg.chat.id.0)
                .bind(topic)
                .bind(msg.id.0)
                .bind(reply)
                .execute(&mut tx)
                .await?;
            tx.commit().await?;
            return Ok(());
        }
        if matches!(command, "/users" | "/promote" | "/demote") {
            let reply = if role != "superuser" {
                "Only a superuser can manage DataDog user roles.".to_owned()
            } else if command == "/users" {
                let rows = db::query("SELECT display_name,username,telegram_user_id,role,status FROM users ORDER BY role DESC,display_name")
                    .fetch_all(&mut tx).await?;
                rows.iter()
                    .map(|r| {
                        format!(
                            "• {}{} · {} · {} · ID {}",
                            r.get::<String, _>("display_name"),
                            r.get::<Option<String>, _>("username")
                                .map(|v| format!(" (@{v})"))
                                .unwrap_or_default(),
                            r.get::<String, _>("role"),
                            r.get::<String, _>("status"),
                            r.get::<i64, _>("telegram_user_id")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                let handle = text
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .trim_start_matches('@')
                    .to_lowercase();
                if handle.is_empty() {
                    format!("Usage: {command} @username")
                } else {
                    let target = db::query("SELECT id,telegram_user_id,display_name,private_chat_id FROM users WHERE lower(username)=? AND status='approved'")
                        .bind(&handle).fetch_optional(&mut tx).await?;
                    if let Some(target) = target {
                        let target_id: i64 = target.get("id");
                        if command == "/demote"
                            && target.get::<i64, _>("telegram_user_id") == user_id
                        {
                            "You cannot demote your own superuser account.".to_owned()
                        } else {
                            let new_role = if command == "/promote" {
                                "superuser"
                            } else {
                                "user"
                            };
                            db::query("UPDATE users SET role=?,updated_at=? WHERE id=?")
                                .bind(new_role)
                                .bind(now())
                                .bind(target_id)
                                .execute(&mut tx)
                                .await?;
                            db::query("INSERT INTO user_access_audit(target_user,actor_user,action,created_at) VALUES(?,?,?,?)")
                                .bind(target_id).bind(access.get::<i64,_>("id")).bind(if command=="/promote"{"promoted_superuser"}else{"demoted_user"}).bind(now()).execute(&mut tx).await?;
                            if let Some(chat) = target.get::<Option<i64>, _>("private_chat_id") {
                                db::query("INSERT INTO outbox(chat,topic,kind,text) VALUES(?,0,'reply',?)").bind(chat).bind(format!("Your DataDog role is now {new_role}.")).execute(&mut tx).await?;
                            }
                            format!(
                                "{} is now a {new_role}.",
                                target.get::<String, _>("display_name")
                            )
                        }
                    } else {
                        "No approved DataDog user has that username.".to_owned()
                    }
                }
            };
            db::query("INSERT INTO outbox(chat,topic,reply_to,kind,text) VALUES(?,?,?,'reply',?)")
                .bind(msg.chat.id.0)
                .bind(topic)
                .bind(msg.id.0)
                .bind(reply)
                .execute(&mut tx)
                .await?;
            tx.commit().await?;
            return Ok(());
        }
        if is_start {
            let can_create = role != "guest"
                || (!msg.chat.is_private()
                    && owner_telegram_id.is_some_and(|owner| owner == user_id));
            let intro = welcome_text(&role, can_create);
            db::query(
                "INSERT INTO outbox(chat,topic,reply_to,kind,text) VALUES(?,?,?,'welcome',?)",
            )
            .bind(msg.chat.id.0)
            .bind(topic)
            .bind(msg.id.0)
            .bind(intro)
            .execute(&mut tx)
            .await?;
            tx.commit().await?;
            return Ok(());
        }
        let active = db::query("SELECT project,inspection_run,awaiting_project_name,listening,pending_project_name,pending_project_message FROM chats WHERE chat=? AND topic=?")
            .bind(msg.chat.id.0)
            .bind(topic)
            .fetch_one(&mut tx)
            .await?;
        let instruction = normalize(text);
        let stop_listening = matches!(
            instruction.as_str(),
            "stop listening" | "pause" | "pause datadog" | "datadog stop listening"
        );
        let start_listening = matches!(
            instruction.as_str(),
            "start listening" | "resume" | "resume datadog" | "datadog start listening"
        );
        if !msg.chat.is_private() && (stop_listening || start_listening) {
            let listening = i64::from(start_listening);
            db::query("UPDATE chats SET listening=? WHERE chat=? AND topic=?")
                .bind(listening)
                .bind(msg.chat.id.0)
                .bind(topic)
                .execute(&mut tx)
                .await?;
            let reply = if start_listening {
                "Listening again. I’ll track relevant photos and inspection notes."
            } else {
                "Paused. I’ll ignore messages and photos until someone says “start listening” or /resume."
            };
            db::query("INSERT INTO outbox(chat,topic,reply_to,kind,text) VALUES(?,?,?,'reply',?)")
                .bind(msg.chat.id.0)
                .bind(topic)
                .bind(msg.id.0)
                .bind(reply)
                .execute(&mut tx)
                .await?;
            tx.commit().await?;
            return Ok(());
        }
        if !msg.chat.is_private() && active.get::<i64, _>("listening") == 0 {
            tx.commit().await?;
            return Ok(());
        }
        let local_command = instruction.as_str();
        if matches!(
            local_command,
            "show checklist" | "show checklist for this project" | "checklist"
        ) {
            db::query("INSERT INTO outbox(chat,topic,reply_to,kind,text) VALUES(?,?,?,'checklist_show','')")
                .bind(msg.chat.id.0).bind(topic).bind(msg.id.0).execute(&mut tx).await?;
            tx.commit().await?;
            return Ok(());
        }
        if matches!(local_command, "show team" | "show team members" | "team") {
            let members = db::query("SELECT u.display_name,u.username,CASE WHEN p.lead_user=u.id THEN 'lead' ELSE 'member' END AS project_role FROM chats c JOIN projects p ON p.id=c.project JOIN project_members pm ON pm.project=p.id JOIN users u ON u.id=pm.user WHERE c.chat=? AND c.topic=? ORDER BY CASE WHEN p.lead_user=u.id THEN 0 ELSE 1 END,u.display_name")
                .bind(msg.chat.id.0).bind(topic).fetch_all(&mut tx).await?;
            let reply = if members.is_empty() {
                "No active project team.".to_owned()
            } else {
                members
                    .into_iter()
                    .map(|row| {
                        let handle = row
                            .get::<Option<String>, _>("username")
                            .map(|v| format!(" (@{v})"))
                            .unwrap_or_default();
                        format!(
                            "• {}{} — {}",
                            row.get::<String, _>("display_name"),
                            handle,
                            row.get::<String, _>("project_role")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            db::query("INSERT INTO outbox(chat,topic,reply_to,kind,text) VALUES(?,?,?,'reply',?)")
                .bind(msg.chat.id.0)
                .bind(topic)
                .bind(msg.id.0)
                .bind(reply)
                .execute(&mut tx)
                .await?;
            tx.commit().await?;
            return Ok(());
        }
        if matches!(
            local_command,
            "whats the current project"
                | "what s the current project"
                | "current project"
                | "which project"
        ) {
            let current = db::query("SELECT p.location,r.report_reference,r.title FROM chats c LEFT JOIN projects p ON p.id=c.project LEFT JOIN inspection_runs r ON r.id=c.inspection_run WHERE c.chat=? AND c.topic=?")
                .bind(msg.chat.id.0).bind(topic).fetch_one(&mut tx).await?;
            let reply = match current.get::<Option<String>, _>("location") {
                Some(location) => format!(
                    "Current project: {location} · {} · {}",
                    current
                        .get::<Option<String>, _>("report_reference")
                        .unwrap_or_else(|| "No reference".into()),
                    current
                        .get::<Option<String>, _>("title")
                        .unwrap_or_else(|| "No active inspection".into())
                ),
                None => "No active project. Say “start a new project Star Vista”.".into(),
            };
            db::query("INSERT INTO outbox(chat,topic,reply_to,kind,text) VALUES(?,?,?,'reply',?)")
                .bind(msg.chat.id.0)
                .bind(topic)
                .bind(msg.id.0)
                .bind(reply)
                .execute(&mut tx)
                .await?;
            tx.commit().await?;
            return Ok(());
        }
        let awaiting_input = active.get::<i64, _>("awaiting_project_name") == 1;
        if !msg.chat.is_private() && !actionable_group_message(msg, text, awaiting_input) {
            tx.commit().await?;
            return Ok(());
        }
        let limit = db::query("SELECT window_started_at,accepted_count FROM message_rate_limits WHERE chat=? AND user=?")
            .bind(msg.chat.id.0).bind(access.get::<i64,_>("id")).fetch_optional(&mut tx).await?;
        let timestamp = now();
        if let Some(limit) = limit {
            let started: i64 = limit.get("window_started_at");
            let count: i64 = limit.get("accepted_count");
            if timestamp - started < 60 && count >= 12 {
                tx.commit().await?;
                return Ok(());
            }
            if timestamp - started < 60 {
                db::query("UPDATE message_rate_limits SET accepted_count=accepted_count+1 WHERE chat=? AND user=?")
                    .bind(msg.chat.id.0).bind(access.get::<i64,_>("id")).execute(&mut tx).await?;
            } else {
                db::query("UPDATE message_rate_limits SET window_started_at=?,accepted_count=1 WHERE chat=? AND user=?")
                    .bind(timestamp).bind(msg.chat.id.0).bind(access.get::<i64,_>("id")).execute(&mut tx).await?;
            }
        } else {
            db::query("INSERT INTO message_rate_limits(chat,user,window_started_at,accepted_count) VALUES(?,?,?,1)")
                .bind(msg.chat.id.0).bind(access.get::<i64,_>("id")).bind(timestamp).execute(&mut tx).await?;
        }
        if let Some(project) = active.get::<Option<String>, _>("project") {
            db::query("INSERT OR IGNORE INTO project_members(project,user,added_by,created_at) SELECT ?,u.id,COALESCE(p.lead_user,u.id),? FROM users u JOIN projects p ON p.id=? WHERE u.id=? AND u.status='approved'")
                .bind(&project).bind(now()).bind(&project).bind(access.get::<i64,_>("id")).execute(&mut tx).await?;
        }
        if awaiting_input
            && matches!(
                local_command,
                "cancel" | "cancel project" | "cancel project creation"
            )
        {
            if let Some(previous) = active.get::<Option<i64>, _>("pending_project_message") {
                db::query("UPDATE messages SET status='ignored' WHERE id=? AND status='held'")
                    .bind(previous)
                    .execute(&mut tx)
                    .await?;
            }
            db::query("UPDATE chats SET awaiting_project_name=0,pending_project_name=NULL,pending_project_user=NULL,pending_project_message=NULL WHERE chat=? AND topic=?")
                .bind(msg.chat.id.0).bind(topic).execute(&mut tx).await?;
            db::query("INSERT INTO outbox(chat,topic,reply_to,kind,text) VALUES(?,?,?,'reply','Project creation cancelled.')")
                .bind(msg.chat.id.0).bind(topic).bind(msg.id.0).execute(&mut tx).await?;
            tx.commit().await?;
            return Ok(());
        }
        let proposed_name = requested_project_name(text).or_else(|| {
            (awaiting_input && looks_like_project_name(text)).then(|| text.trim().to_owned())
        });
        if let Some(name) = proposed_name {
            let display_name = title_case_name(&name);
            ensure!(
                !normalize(&display_name).is_empty() && display_name.len() <= 200,
                "invalid project name"
            );
            if let Some(previous) = active.get::<Option<i64>, _>("pending_project_message") {
                db::query("UPDATE messages SET status='ignored' WHERE id=? AND status='held'")
                    .bind(previous)
                    .execute(&mut tx)
                    .await?;
            }
            db::query("INSERT OR IGNORE INTO messages(chat,topic,telegram_id,sender,sent_at,received_at,raw,text,project,inspection_run,status) VALUES(?,?,?,?,?,?,?,?,?,?,'held')")
                .bind(msg.chat.id.0).bind(topic).bind(msg.id.0).bind(sender).bind(msg.date.timestamp()).bind(now()).bind(raw).bind(format!("new project, {display_name}")).bind(active.get::<Option<String>,_>("project")).bind(active.get::<Option<String>,_>("inspection_run")).execute(&mut tx).await?;
            let pending: i64 =
                db::query_scalar("SELECT id FROM messages WHERE chat=? AND telegram_id=?")
                    .bind(msg.chat.id.0)
                    .bind(msg.id.0)
                    .fetch_one(&mut tx)
                    .await?;
            db::query("UPDATE chats SET pending_project_name=?,pending_project_user=?,pending_project_message=?,awaiting_project_name=1 WHERE chat=? AND topic=?")
                .bind(&display_name).bind(access.get::<i64,_>("id")).bind(pending).bind(msg.chat.id.0).bind(topic).execute(&mut tx).await?;
            db::query("INSERT INTO outbox(chat,topic,reply_to,kind,text) VALUES(?,?,?,'project_confirmation',?)")
                .bind(msg.chat.id.0).bind(topic).bind(msg.id.0).bind(format!("Create project “{display_name}”?\n\nTap Create, type a different name, or cancel.")).execute(&mut tx).await?;
            tx.commit().await?;
            return Ok(());
        }
        let effective_text = text.to_owned();
        let inserted = db::query("INSERT OR IGNORE INTO messages(chat,topic,telegram_id,sender,sent_at,received_at,raw,text,project,inspection_run,status) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
      .bind(msg.chat.id.0).bind(topic).bind(msg.id.0).bind(sender).bind(msg.date.timestamp()).bind(now()).bind(raw).bind(effective_text).bind(active.get::<Option<String>,_>("project")).bind(active.get::<Option<String>,_>("inspection_run")).bind(if crate::worker::image_file(msg).is_some(){"receiving"}else{"pending"}).execute(&mut tx).await?;
        if inserted.rows_affected() == 1 {
            let source_message: i64 =
                db::query_scalar("SELECT id FROM messages WHERE chat=? AND telegram_id=?")
                    .bind(msg.chat.id.0)
                    .bind(msg.id.0)
                    .fetch_one(&mut tx)
                    .await?;
            db::query("INSERT INTO outbox(chat,topic,reply_to,kind,text,source_message) VALUES(?,?,?,'loading','Fetching.',?)")
                .bind(msg.chat.id.0).bind(topic).bind(msg.id.0).bind(source_message).execute(&mut tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn accept_legal(
        &self,
        actor_telegram_id: i64,
        target_telegram_id: i64,
        username: Option<&str>,
        display_name: &str,
        chat: Option<i64>,
        topic: i64,
        send_welcome: bool,
    ) -> Result<String> {
        ensure!(
            actor_telegram_id == target_telegram_id,
            "Accept for your own account"
        );
        let mut tx = self.db.begin().await?;
        let mut user = db::query(
            "SELECT id,role,status,display_name,username,terms_version,privacy_version FROM users WHERE telegram_user_id=?",
        )
            .bind(actor_telegram_id)
            .fetch_optional(&mut tx)
            .await?;
        if user.is_none() {
            let username = username.map(str::to_lowercase);
            let created = now();
            db::query("INSERT INTO users(telegram_user_id,username,display_name,role,status,requested_at,created_at,updated_at) VALUES(?,?,?,'guest','pending',?,?,?)")
                .bind(actor_telegram_id).bind(&username).bind(display_name).bind(created).bind(created).bind(created).execute(&mut tx).await?;
            user = db::query("SELECT id,role,status,display_name,username,terms_version,privacy_version FROM users WHERE telegram_user_id=?")
                .bind(actor_telegram_id).fetch_optional(&mut tx).await?;
        }
        let user = user.context("could not create user")?;
        ensure!(
            user.get::<String, _>("status") != "rejected",
            "access is unavailable"
        );
        let already_accepted = user.get::<Option<String>, _>("terms_version").as_deref()
            == Some(TERMS_VERSION)
            && user.get::<Option<String>, _>("privacy_version").as_deref() == Some(PRIVACY_VERSION);
        let user_id: i64 = user.get("id");
        if !already_accepted {
            db::query("UPDATE users SET role=CASE WHEN role='guest' THEN 'user' ELSE role END,status='approved',terms_version=?,privacy_version=?,legal_accepted_at=?,approved_at=COALESCE(approved_at,?),updated_at=? WHERE id=?")
                .bind(TERMS_VERSION).bind(PRIVACY_VERSION).bind(now()).bind(now()).bind(now()).bind(user_id).execute(&mut tx).await?;
            db::query("INSERT INTO user_access_audit(target_user,actor_user,action,created_at) VALUES(?,?, 'legal_accepted',?)")
                .bind(user_id).bind(user_id).bind(now()).execute(&mut tx).await?;
        }
        let registration = format!(
            "New DataDog registration\n{}{}\nTelegram ID: {}",
            user.get::<String, _>("display_name"),
            user.get::<Option<String>, _>("username")
                .map(|name| format!(" · @{name}"))
                .unwrap_or_default(),
            actor_telegram_id
        );
        if !already_accepted {
            let admins=db::query("SELECT private_chat_id FROM users WHERE role='superuser' AND status='approved' AND private_chat_id IS NOT NULL AND telegram_user_id<>?")
                .bind(actor_telegram_id).fetch_all(&mut tx).await?;
            for admin in admins {
                db::query("INSERT INTO outbox(chat,topic,kind,text) VALUES(?,0,'reply',?)")
                    .bind(admin.get::<i64, _>("private_chat_id"))
                    .bind(&registration)
                    .execute(&mut tx)
                    .await?;
            }
        }
        if let Some(chat) = chat {
            db::query("INSERT INTO chat_participants(chat,topic,user,first_seen_at,last_seen_at) VALUES(?,?,?,?,?) ON CONFLICT(chat,topic,user) DO UPDATE SET last_seen_at=excluded.last_seen_at")
                .bind(chat).bind(topic).bind(user_id).bind(now()).bind(now()).execute(&mut tx).await?;
            let role: String = if user.get::<String, _>("role") == "guest" {
                "user".into()
            } else {
                user.get("role")
            };
            let can_create = true;
            if can_create {
                db::query("UPDATE chats SET awaiting_project_name=CASE WHEN project IS NULL THEN 1 ELSE 0 END WHERE chat=? AND topic=?")
                    .bind(chat).bind(topic).execute(&mut tx).await?;
            }
            db::query("INSERT OR IGNORE INTO project_members(project,user,added_by,created_at) SELECT DISTINCT c.project,?,COALESCE(p.lead_user,?),? FROM chats c JOIN projects p ON p.id=c.project WHERE c.chat=? AND c.project IS NOT NULL")
                .bind(user_id).bind(user_id).bind(now()).bind(chat).execute(&mut tx).await?;
            if !already_accepted && send_welcome {
                db::query("INSERT INTO outbox(chat,topic,kind,text) VALUES(?,?,'welcome',?)")
                    .bind(chat)
                    .bind(topic)
                    .bind(welcome_text(&role, can_create))
                    .execute(&mut tx)
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(if already_accepted {
            "You’re already registered and on this team."
        } else {
            "Accepted. You’re now a team member."
        }
        .into())
    }
    pub async fn resolve_project_confirmation(
        &self,
        actor_telegram_id: i64,
        chat: i64,
        topic: i64,
        confirm: bool,
    ) -> Result<String> {
        let mut tx = self.db.begin().await?;
        let row = db::query("SELECT c.pending_project_name,c.pending_project_message,c.pending_project_user,u.id AS actor_id FROM chats c JOIN users u ON u.telegram_user_id=? AND u.status='approved' WHERE c.chat=? AND c.topic=?")
            .bind(actor_telegram_id).bind(chat).bind(topic).fetch_one(&mut tx).await?;
        let actor_id: i64 = row.get("actor_id");
        ensure!(
            row.get::<Option<i64>, _>("pending_project_user") == Some(actor_id),
            "Only the person who proposed this project can confirm it"
        );
        let message = row
            .get::<Option<i64>, _>("pending_project_message")
            .context("No project is awaiting confirmation")?;
        let name = row
            .get::<Option<String>, _>("pending_project_name")
            .context("No project is awaiting confirmation")?;
        if confirm {
            db::query("UPDATE messages SET status='pending' WHERE id=? AND status='held'")
                .bind(message)
                .execute(&mut tx)
                .await?;
            db::query("INSERT INTO outbox(chat,topic,kind,text,source_message) VALUES(?,?,'loading','Fetching.',?)")
                .bind(chat).bind(topic).bind(message).execute(&mut tx).await?;
            db::query("UPDATE chats SET awaiting_project_name=0,pending_project_name=NULL,pending_project_user=NULL,pending_project_message=NULL WHERE chat=? AND topic=?")
                .bind(chat).bind(topic).execute(&mut tx).await?;
            tx.commit().await?;
            Ok(format!("Creating {name}."))
        } else {
            db::query("UPDATE messages SET status='ignored' WHERE id=? AND status='held'")
                .bind(message)
                .execute(&mut tx)
                .await?;
            db::query("UPDATE chats SET awaiting_project_name=0,pending_project_name=NULL,pending_project_user=NULL,pending_project_message=NULL WHERE chat=? AND topic=?")
                .bind(chat).bind(topic).execute(&mut tx).await?;
            tx.commit().await?;
            Ok("Project creation cancelled.".into())
        }
    }
    pub async fn next_job(&self) -> Result<Option<Job>> {
        Ok(db::query_as("SELECT id,chat,topic,telegram_id,sender,text,raw,project,inspection_run,attempts,analysis FROM messages WHERE status='pending' AND next_attempt<=? AND NOT EXISTS(SELECT 1 FROM messages earlier WHERE earlier.id<messages.id AND earlier.chat=messages.chat AND earlier.topic=messages.topic AND earlier.status='receiving') ORDER BY id LIMIT 1").bind(now()).fetch_optional(&self.db).await?)
    }
    pub async fn evidence(&self, id: i64) -> Result<Option<Evidence>> {
        Ok(db::query_as("SELECT id,message,project,inspection_run,elevation,finding,label,description,path,preview_path,yolo_attempts FROM evidence WHERE id=?").bind(id).fetch_optional(&self.db).await?)
    }
    pub async fn current_evidence(&self, job: &Job) -> Result<Option<Evidence>> {
        let id: Option<i64> = db::query_scalar("SELECT id FROM evidence WHERE message=?")
            .bind(job.id)
            .fetch_optional(&self.db)
            .await?;
        if let Some(id) = id {
            self.evidence(id).await
        } else {
            Ok(None)
        }
    }
    pub async fn reserve_evidence(&self, job: &Job) -> Result<i64> {
        let project = self.active_project(job).await?;
        let inspection_run = self.active_inspection(job, project.as_deref()).await?;
        db::query("INSERT OR IGNORE INTO evidence(message,project,inspection_run,created_at,captured_at) VALUES(?,?,?,?,?)")
            .bind(job.id)
            .bind(&project)
            .bind(&inspection_run)
            .bind(now())
            .bind(now())
            .execute(&self.db)
            .await?;
        Ok(db::query_scalar("SELECT id FROM evidence WHERE message=?")
            .bind(job.id)
            .fetch_one(&self.db)
            .await?)
    }
    pub async fn active_project(&self, job: &Job) -> Result<Option<String>> {
        let candidate = if job.project.is_some() {
            job.project.clone()
        } else {
            db::query_scalar::<Option<String>>("SELECT project FROM chats WHERE chat=? AND topic=?")
                .bind(job.chat)
                .bind(job.topic)
                .fetch_optional(&self.db)
                .await?
                .flatten()
        };
        let Some(project) = candidate else {
            return Ok(None);
        };
        let telegram_id = job
            .sender
            .strip_prefix("user:")
            .and_then(|v| v.parse::<i64>().ok())
            .context("invalid message sender")?;
        let allowed: i64 = db::query_scalar("SELECT EXISTS(SELECT 1 FROM users u JOIN projects p ON p.id=? WHERE u.telegram_user_id=? AND u.status='approved' AND (u.role='superuser' OR p.lead_user=u.id OR EXISTS(SELECT 1 FROM project_members pm WHERE pm.project=p.id AND pm.user=u.id)))")
            .bind(&project).bind(telegram_id).fetch_one(&self.db).await?;
        Ok((allowed == 1).then_some(project))
    }
    pub async fn active_inspection(
        &self,
        job: &Job,
        project: Option<&str>,
    ) -> Result<Option<String>> {
        let Some(project) = project else {
            return Ok(None);
        };
        let candidate = if job.inspection_run.is_some() {
            job.inspection_run.clone()
        } else {
            db::query_scalar::<Option<String>>(
                "SELECT inspection_run FROM chats WHERE chat=? AND topic=?",
            )
            .bind(job.chat)
            .bind(job.topic)
            .fetch_optional(&self.db)
            .await?
            .flatten()
        };
        if let Some(run) = candidate {
            let valid: i64 = db::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM inspection_runs WHERE id=? AND project=?)",
            )
            .bind(&run)
            .bind(project)
            .fetch_one(&self.db)
            .await?;
            if valid == 1 {
                return Ok(Some(run));
            }
        }
        Ok(db::query_scalar("SELECT id FROM inspection_runs WHERE project=? AND status<>'archived' ORDER BY run_number DESC LIMIT 1")
            .bind(project)
            .fetch_optional(&self.db)
            .await?)
    }
    pub async fn context(&self, job: &Job) -> Result<Value> {
        let project = self.active_project(job).await?;
        let inspection_run = self.active_inspection(job, project.as_deref()).await?;
        // Freeze a job's project and inspection before inference so retries cannot move evidence.
        if let Some(p) = &project {
            db::query("UPDATE messages SET project=COALESCE(project,?),inspection_run=COALESCE(inspection_run,?) WHERE id=?")
                .bind(p)
                .bind(&inspection_run)
                .bind(job.id)
                .execute(&self.db)
                .await?;
            db::query("UPDATE evidence SET project=COALESCE(project,?),inspection_run=COALESCE(inspection_run,?) WHERE message=?")
                .bind(p).bind(&inspection_run).bind(job.id).execute(&self.db).await?;
        }
        let telegram_id = job
            .sender
            .strip_prefix("user:")
            .and_then(|v| v.parse::<i64>().ok())
            .context("invalid message sender")?;
        let projects=db::query("SELECT p.id,p.location,p.archived FROM projects p JOIN users u ON u.telegram_user_id=? WHERE u.role='superuser' OR p.lead_user=u.id OR EXISTS(SELECT 1 FROM project_members pm WHERE pm.project=p.id AND pm.user=u.id) ORDER BY p.created_at DESC LIMIT 100").bind(telegram_id).fetch_all(&self.db).await?;
        let projects:Vec<Value>=projects.iter().map(|r|json!({"id":r.get::<String,_>("id"),"location":r.get::<String,_>("location"),"archived":r.get::<i64,_>("archived")==1})).collect();
        let context=db::query("SELECT project,finding,evidence,prompt FROM contexts WHERE chat=? AND topic=? AND sender=?").bind(job.chat).bind(job.topic).bind(&job.sender).fetch_optional(&self.db).await?;
        let mut sender_context = Value::Null;
        let mut pending = Value::Null;
        if let Some(c) = context {
            if c.get::<Option<String>, _>("project") == project {
                let finding = c.get::<Option<i64>, _>("finding");
                sender_context = json!({"finding":self.finding_json(finding).await?,"question":c.get::<Option<String>,_>("prompt")});
                if let Some(id) = c.get::<Option<i64>, _>("evidence") {
                    pending = self.evidence_json(id).await?;
                }
            }
        }
        let raw: Value = serde_json::from_str(&job.raw)?;
        let reply_id = raw["reply_to_message"]["message_id"].as_i64();
        let mut reply = Value::Null;
        if let Some(reply_id) = reply_id {
            let id:Option<i64>=db::query_scalar("SELECT e.id FROM evidence e JOIN messages m ON m.id=e.message WHERE m.chat=? AND m.topic=? AND m.telegram_id=? UNION ALL SELECT evidence FROM outbox WHERE chat=? AND topic=? AND telegram_message=? AND evidence IS NOT NULL LIMIT 1")
      .bind(job.chat).bind(job.topic).bind(reply_id).bind(job.chat).bind(job.topic).bind(reply_id).fetch_optional(&self.db).await?;
            if let Some(id) = id {
                reply = self.evidence_json(id).await?;
            }
        }
        let current = if let Some(e) = self.current_evidence(job).await? {
            self.evidence_json(e.id).await?
        } else {
            Value::Null
        };
        let recent=db::query("SELECT text FROM messages WHERE chat=? AND topic=? AND sender=? AND id<? AND text<>'' ORDER BY id DESC LIMIT 4").bind(job.chat).bind(job.topic).bind(&job.sender).bind(job.id).fetch_all(&self.db).await?;
        let recent: Vec<String> = recent.iter().map(|r| r.get("text")).collect();
        let inspection_history = if let Some(project) = &project {
            db::query("SELECT r.id,r.run_number,r.title,r.report_reference,r.status,r.current_step,r.inspection_started_at,r.inspection_ended_at,(SELECT MAX(version) FROM report_drafts d WHERE d.inspection_run=r.id) AS latest_version FROM inspection_runs r WHERE r.project=? ORDER BY r.run_number DESC LIMIT 20")
                .bind(project).fetch_all(&self.db).await?
                .iter().map(|r| json!({
                    "id":r.get::<String,_>("id"),"run_number":r.get::<i64,_>("run_number"),
                    "title":r.get::<String,_>("title"),"report_reference":r.get::<Option<String>,_>("report_reference"),
                    "status":r.get::<String,_>("status"),"current_step":r.get::<String,_>("current_step"),
                    "inspection_started_at":r.get::<Option<i64>,_>("inspection_started_at"),
                    "inspection_ended_at":r.get::<Option<i64>,_>("inspection_ended_at"),
                    "latest_report_version":r.get::<Option<i64>,_>("latest_version")
                })).collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        // Only user-authored descriptions are carried across inspections. They are
        // explicitly marked historical so old conditions cannot become current findings.
        let historical_findings = if let (Some(project), Some(active_run)) =
            (&project, &inspection_run)
        {
            db::query("SELECT r.run_number,r.report_reference,f.number,el.name AS elevation,d.text FROM descriptions d JOIN findings f ON f.id=d.finding JOIN inspection_runs r ON r.id=f.inspection_run JOIN elevations el ON el.id=f.elevation WHERE r.project=? AND r.id<>? AND d.text<>'' ORDER BY r.run_number DESC,f.number LIMIT 100")
                .bind(project).bind(active_run).fetch_all(&self.db).await?.iter().map(|r|json!({"historical":true,"inspection_number":r.get::<i64,_>("run_number"),"report_reference":r.get::<Option<String>,_>("report_reference"),"finding":format!("F{:03}",r.get::<i64,_>("number")),"elevation":r.get::<String,_>("elevation"),"user_description":r.get::<String,_>("text")})).collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let active_inspection = if let Some(run) = &inspection_run {
            let r=db::query("SELECT id,run_number,title,report_reference,status,current_step,notice_reference,inspection_started_at,inspection_ended_at,report_due_at,metadata FROM inspection_runs WHERE id=?")
                .bind(run).fetch_one(&self.db).await?;
            let steps=db::query("SELECT step_key,ordinal,title,status,required FROM inspection_steps WHERE inspection_run=? ORDER BY ordinal")
                .bind(run).fetch_all(&self.db).await?.iter().map(|s|json!({"key":s.get::<String,_>("step_key"),"ordinal":s.get::<i64,_>("ordinal"),"title":s.get::<String,_>("title"),"status":s.get::<String,_>("status"),"required":s.get::<i64,_>("required")==1})).collect::<Vec<_>>();
            json!({"id":r.get::<String,_>("id"),"run_number":r.get::<i64,_>("run_number"),"title":r.get::<String,_>("title"),"report_reference":r.get::<Option<String>,_>("report_reference"),"status":r.get::<String,_>("status"),"current_step":r.get::<String,_>("current_step"),"notice_reference":r.get::<Option<String>,_>("notice_reference"),"inspection_started_at":r.get::<Option<i64>,_>("inspection_started_at"),"inspection_ended_at":r.get::<Option<i64>,_>("inspection_ended_at"),"report_due_at":r.get::<Option<i64>,_>("report_due_at"),"steps":steps})
        } else {
            Value::Null
        };
        Ok(
            json!({"active_project":project,"active_inspection":active_inspection,"inspection_history":inspection_history,"historical_findings":historical_findings,"projects":projects,"sender_context":sender_context,"pending_evidence":pending,"reply_evidence":reply,"current_evidence":current,"checklist":self.checklist(project.as_deref(),inspection_run.as_deref()).await?,"recent_sender_messages":recent,"reply_text":raw["reply_to_message"]["text"].as_str().or_else(||raw["reply_to_message"]["caption"].as_str())}),
        )
    }
    async fn finding_json(&self, id: Option<i64>) -> Result<Value> {
        if let Some(id) = id {
            if let Some(r)=db::query("SELECT f.number,f.level,e.name FROM findings f JOIN elevations e ON e.id=f.elevation WHERE f.id=?").bind(id).fetch_optional(&self.db).await? {return Ok(json!({"id":format!("F{:03}",r.get::<i64,_>("number")),"elevation":r.get::<String,_>("name"),"level":r.get::<Option<String>,_>("level")}));}
        }
        Ok(Value::Null)
    }
    async fn evidence_json(&self, id: i64) -> Result<Value> {
        if let Some(e) = self.evidence(id).await? {
            let name: Option<String> = if let Some(id) = e.elevation {
                db::query_scalar("SELECT name FROM elevations WHERE id=?")
                    .bind(id)
                    .fetch_optional(&self.db)
                    .await?
            } else {
                None
            };
            return Ok(
                json!({"id":format!("E{:04}",e.id),"project":e.project,"inspection_run":e.inspection_run,"label":e.label,"finding":self.finding_json(e.finding).await?,"elevation":name,"has_photo":e.path.is_some(),"description":e.description}),
            );
        }
        Ok(Value::Null)
    }
    pub async fn save_analysis(
        &self,
        job: &Job,
        analysis: &crate::deepseek::Analysis,
    ) -> Result<()> {
        db::query("UPDATE messages SET analysis=?,model=?,usage=?,error=NULL WHERE id=?")
            .bind(serde_json::to_string(&analysis.proposal)?)
            .bind(&analysis.model)
            .bind(analysis.usage.to_string())
            .bind(job.id)
            .execute(&self.db)
            .await?;
        Ok(())
    }
    pub async fn retry(&self, job: &Job, error: &str) -> Result<()> {
        let attempts = job.attempts + 1;
        let status = if attempts >= 5 { "failed" } else { "pending" };
        db::query("UPDATE messages SET attempts=?,next_attempt=?,status=?,error=? WHERE id=?")
            .bind(attempts)
            .bind(now() + 30 * 2i64.pow(attempts.min(6) as u32))
            .bind(status)
            .bind(error)
            .bind(job.id)
            .execute(&self.db)
            .await?;
        if attempts == 1 {
            self.enqueue(job,"reply","Your message is saved. Classification is temporarily unavailable; I’ll retry. You can label a saved photo by replying with ‘building overview’, ‘plan’, ‘East elevation overview’, or ‘F001 close-up’.",None,None).await?;
        }
        if attempts == 5 {
            self.enqueue(job,"reply","Automatic classification is still unavailable. Your evidence remains saved. Please reply to the photo with a label, or resend the project instruction when the service is available.",None,None).await?;
        }
        Ok(())
    }
    pub async fn enqueue(
        &self,
        job: &Job,
        kind: &str,
        text: &str,
        path: Option<&str>,
        evidence: Option<i64>,
    ) -> Result<()> {
        db::query(
            "INSERT INTO outbox(chat,topic,reply_to,kind,text,path,evidence,source_message) VALUES(?,?,?,?,?,?,?,?)",
        )
        .bind(job.chat)
        .bind(job.topic)
        .bind(job.telegram_id)
        .bind(kind)
        .bind(text)
        .bind(path)
        .bind(evidence)
        .bind(job.id)
        .execute(&self.db)
        .await?;
        Ok(())
    }
    pub async fn apply(&self, job: &Job, p: &Proposal, context: &Value) -> Result<()> {
        if p.action == Action::Ignore && self.current_evidence(job).await?.is_none() {
            self.enqueue(
                job,
                "reply",
                &format!(
                    "I’m not sure what you want me to do. Did you mean to show the checklist, organise evidence, or generate a report?\n\n{}",
                    help_text()
                ),
                None,
                None,
            )
            .await?;
            db::query("UPDATE messages SET status='done' WHERE id=?")
                .bind(job.id)
                .execute(&self.db)
                .await?;
            return Ok(());
        }
        let mut tx = self.db.begin().await?;
        let actor_telegram_id = job
            .sender
            .strip_prefix("user:")
            .and_then(|v| v.parse::<i64>().ok())
            .context("invalid message sender")?;
        let actor =
            db::query("SELECT id,role FROM users WHERE telegram_user_id=? AND status='approved'")
                .bind(actor_telegram_id)
                .fetch_one(&mut tx)
                .await?;
        let actor_id: i64 = actor.get("id");
        let actor_role: String = actor.get("role");
        let is_superuser = actor_role == "superuser";
        let mut project = context["active_project"].as_str().map(str::to_owned);
        let mut inspection_run = context["active_inspection"]["id"]
            .as_str()
            .map(str::to_owned);
        let mut receipt = String::new();
        let mut question = None;
        let mut changed = false;
        let mut auto_report = false;
        let mut finding = None;
        let current = self.current_evidence(job).await?;
        let target_id = current
            .as_ref()
            .filter(|e| e.path.is_some())
            .map(|e| e.id)
            .or_else(|| {
                p.evidence
                    .as_deref()
                    .and_then(|s| parse_id(s, 'E'))
                    .or_else(|| {
                        context["reply_evidence"]["id"]
                            .as_str()
                            .and_then(|s| parse_id(s, 'E'))
                    })
                    .or_else(|| {
                        if current.is_none() {
                            context["pending_evidence"]["id"]
                                .as_str()
                                .and_then(|s| parse_id(s, 'E'))
                        } else {
                            None
                        }
                    })
                    .or(current.as_ref().map(|e| e.id))
            });
        let mut target = None;
        if let Some(id) = target_id {
            target=db::query_as::<Evidence>("SELECT e.id,e.message,e.project,e.inspection_run,e.elevation,e.finding,e.label,e.description,e.path,e.preview_path,e.yolo_attempts FROM evidence e JOIN messages m ON m.id=e.message WHERE e.id=? AND m.chat=? AND m.topic=?")
       .bind(id).bind(job.chat).bind(job.topic).fetch_optional(&mut tx).await?;
            ensure!(target.is_some(), "evidence is outside this chat/topic");
        }
        match p.action {
            Action::SelectProject => {
                if let Some(location) = p.location.as_deref().filter(|v| !normalize(v).is_empty()) {
                    let key = normalize(location);
                    ensure!(key.len() <= 200, "location too long");
                    let existing: Option<String> = db::query_scalar(
                        "SELECT p.id FROM projects p LEFT JOIN aliases a ON a.project=p.id WHERE (p.location_key=? OR (a.chat=? AND a.location_key=?)) AND (? OR p.lead_user=? OR EXISTS(SELECT 1 FROM project_members pm WHERE pm.project=p.id AND pm.user=?)) LIMIT 1",
                    )
                    .bind(&key).bind(job.chat).bind(&key).bind(is_superuser).bind(actor_id).bind(actor_id)
                    .fetch_optional(&mut tx)
                    .await?;
                    if let Some(id) = existing {
                        project = Some(id);
                    } else {
                        let sites=db::query("SELECT p.id,p.location,p.location_key FROM projects p WHERE p.archived=0 AND (? OR p.lead_user=? OR EXISTS(SELECT 1 FROM project_members pm WHERE pm.project=p.id AND pm.user=?))").bind(is_superuser).bind(actor_id).bind(actor_id).fetch_all(&mut tx).await?;
                        let similar: Vec<String> = sites
                            .iter()
                            .filter(|r| {
                                strsim::jaro_winkler(&key, &r.get::<String, _>("location_key"))
                                    > 0.88
                            })
                            .map(|r| r.get("location"))
                            .collect();
                        if !similar.is_empty()
                            && !job.text.to_lowercase().contains(&format!(
                                "new distinct location: {}",
                                location.to_lowercase()
                            ))
                        {
                            question = Some(format!(
                                "Did you mean {}? Please use the full existing location, or say ‘new distinct location: {location}’.",
                                similar.join(" or ")
                            ));
                        } else {
                            let chat=db::query("SELECT owner_telegram_id,is_private FROM chats WHERE chat=? AND topic=?").bind(job.chat).bind(job.topic).fetch_one(&mut tx).await?;
                            let is_private = chat.get::<i64, _>("is_private") == 1;
                            let owner_telegram_id = chat.get::<Option<i64>, _>("owner_telegram_id");
                            let can_create = actor_role != "guest"
                                || (!is_private && owner_telegram_id == Some(actor_telegram_id));
                            if !can_create {
                                question=Some("Only a project creator or this group’s owner can create a project. You can contribute to projects where you are a team member.".into());
                            } else {
                                let lead_id = if is_private {
                                    Some(actor_id)
                                } else if let Some(owner) = owner_telegram_id {
                                    db::query_scalar::<i64>("SELECT id FROM users WHERE telegram_user_id=? AND status='approved'").bind(owner).fetch_optional(&mut tx).await?.or(Some(actor_id))
                                } else {
                                    Some(actor_id)
                                };
                                if let Some(lead_id) = lead_id {
                                    let display_location = title_case_name(location);
                                    let id = uuid::Uuid::new_v4().to_string();
                                    db::query("INSERT INTO projects(id,location,location_key,owner_chat,lead_user,created_at) VALUES(?,?,?,?,?,?)").bind(&id).bind(&display_location).bind(&key).bind(job.chat).bind(lead_id).bind(now()).execute(&mut tx).await?;
                                    db::query("INSERT INTO project_members(project,user,added_by,created_at) VALUES(?,?,?,?)").bind(&id).bind(lead_id).bind(lead_id).bind(now()).execute(&mut tx).await?;
                                    db::query("INSERT OR IGNORE INTO project_members(project,user,added_by,created_at) VALUES(?,?,?,?)").bind(&id).bind(actor_id).bind(lead_id).bind(now()).execute(&mut tx).await?;
                                    db::query("INSERT OR IGNORE INTO project_members(project,user,added_by,created_at) SELECT ?,cp.user,?,? FROM chat_participants cp JOIN users u ON u.id=cp.user WHERE cp.chat=? AND cp.topic=? AND u.status='approved'")
                                        .bind(&id).bind(lead_id).bind(now()).bind(job.chat).bind(job.topic).execute(&mut tx).await?;
                                    db::query("INSERT INTO aliases(chat,location_key,project) VALUES(?,?,?)")
                                        .bind(job.chat).bind(&key).bind(&id).execute(&mut tx).await?;
                                    project = Some(id);
                                    receipt.push_str(&format!("Created {display_location}. "));
                                } else {
                                    question=Some("The group owner must accept DataDog’s terms before this project can be created.".into());
                                }
                            }
                        }
                    }
                    if question.is_none() {
                        let id = project.as_ref().unwrap();
                        let selected_location: String =
                            db::query_scalar("SELECT location FROM projects WHERE id=?")
                                .bind(id)
                                .fetch_one(&mut tx)
                                .await?;
                        let archived: i64 =
                            db::query_scalar("SELECT archived FROM projects WHERE id=?")
                                .bind(id)
                                .fetch_one(&mut tx)
                                .await?;
                        if archived == 1 {
                            question = Some(
                                "This project is archived. Ask me to restore it first.".into(),
                            );
                            project = context["active_project"].as_str().map(str::to_owned);
                        } else {
                            let run: Option<String> = db::query_scalar("SELECT id FROM inspection_runs WHERE project=? AND status<>'archived' ORDER BY run_number DESC LIMIT 1")
                                .bind(id).fetch_optional(&mut tx).await?;
                            let run = match run {
                                Some(run) => run,
                                None => {
                                    create_inspection_run(&mut tx, id, actor_id, None, None).await?
                                }
                            };
                            inspection_run = Some(run.clone());
                            db::query("INSERT INTO chats(chat,topic,project,inspection_run) VALUES(?,?,?,?) ON CONFLICT(chat,topic) DO UPDATE SET project=excluded.project,inspection_run=excluded.inspection_run")
               .bind(job.chat).bind(job.topic).bind(id).bind(&run).execute(&mut tx).await?;
                            receipt.push_str(&format!("Selected {selected_location}."));
                            changed = true;
                            if context["active_project"].as_str() != Some(id.as_str()) {
                                receipt.push_str("\n\nNext: track North, South, East and West elevations\nThen send the building overview, plan, and one overview per elevation.");
                                db::query("DELETE FROM contexts WHERE chat=? AND topic=?")
                                    .bind(job.chat)
                                    .bind(job.topic)
                                    .execute(&mut tx)
                                    .await?;
                            }
                        }
                    }
                } else {
                    question = Some("What is the project location or building name?".into());
                }
            }
            Action::ListProjects => {
                let sites = db::query(
                    "SELECT p.location,p.archived,CASE WHEN p.lead_user=? THEN 1 ELSE 0 END AS is_lead FROM projects p WHERE ? OR p.lead_user=? OR EXISTS(SELECT 1 FROM project_members pm WHERE pm.project=p.id AND pm.user=?) ORDER BY p.location",
                )
                .bind(actor_id).bind(is_superuser).bind(actor_id).bind(actor_id)
                .fetch_all(&mut tx)
                .await?;
                receipt = if sites.is_empty() {
                    "No projects yet. Say ‘new project, Star Vista’.".into()
                } else {
                    sites
                        .iter()
                        .map(|r| {
                            format!(
                                "• {}{}",
                                r.get::<String, _>("location"),
                                match (r.get::<i64, _>("archived"), r.get::<i64, _>("is_lead")) {
                                    (1, _) => " (archived)",
                                    (_, 1) => " (lead)",
                                    _ => " (team)",
                                }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                };
            }
            Action::CreateInspection => {
                let id = project
                    .clone()
                    .context("select a project before creating an inspection")?;
                let run = create_inspection_run(
                    &mut tx,
                    &id,
                    actor_id,
                    p.inspection_title.as_deref(),
                    p.inspection_reference.as_deref(),
                )
                .await?;
                inspection_run = Some(run.clone());
                db::query("INSERT INTO chats(chat,topic,project,inspection_run) VALUES(?,?,?,?) ON CONFLICT(chat,topic) DO UPDATE SET project=excluded.project,inspection_run=excluded.inspection_run")
                    .bind(job.chat).bind(job.topic).bind(&id).bind(&run).execute(&mut tx).await?;
                let row = db::query(
                    "SELECT run_number,title,report_reference FROM inspection_runs WHERE id=?",
                )
                .bind(&run)
                .fetch_one(&mut tx)
                .await?;
                receipt = format!(
                    "Created and selected inspection {} · {}{}.",
                    row.get::<i64, _>("run_number"),
                    row.get::<String, _>("title"),
                    row.get::<Option<String>, _>("report_reference")
                        .map(|v| format!(" · {v}"))
                        .unwrap_or_default()
                );
                changed = true;
            }
            Action::SelectInspection => {
                let id = project
                    .clone()
                    .context("select a project before selecting an inspection")?;
                let reference = p
                    .inspection_reference
                    .as_deref()
                    .or(p.inspection_title.as_deref())
                    .context(
                        "which inspection number, report reference, or title should I select?",
                    )?;
                let number = reference.trim().trim_start_matches('#').parse::<i64>().ok();
                let row=db::query("SELECT id,run_number,title,report_reference FROM inspection_runs WHERE project=? AND status<>'archived' AND (report_reference=? OR lower(title)=lower(?) OR run_number=?) ORDER BY run_number DESC LIMIT 1")
                    .bind(&id).bind(reference.trim()).bind(reference.trim()).bind(number).fetch_optional(&mut tx).await?;
                if let Some(row) = row {
                    let run: String = row.get("id");
                    inspection_run = Some(run.clone());
                    db::query(
                        "UPDATE chats SET inspection_run=? WHERE chat=? AND topic=? AND project=?",
                    )
                    .bind(&run)
                    .bind(job.chat)
                    .bind(job.topic)
                    .bind(&id)
                    .execute(&mut tx)
                    .await?;
                    receipt = format!(
                        "Selected inspection {} · {}{}.",
                        row.get::<i64, _>("run_number"),
                        row.get::<String, _>("title"),
                        row.get::<Option<String>, _>("report_reference")
                            .map(|v| format!(" · {v}"))
                            .unwrap_or_default()
                    );
                    changed = true;
                } else {
                    question=Some("I could not find that inspection for this project. Ask me to list inspections.".into());
                }
            }
            Action::ListInspections => {
                let id = project
                    .clone()
                    .context("select a project before listing inspections")?;
                let rows=db::query("SELECT id,run_number,title,report_reference,status,current_step FROM inspection_runs WHERE project=? ORDER BY run_number DESC").bind(&id).fetch_all(&mut tx).await?;
                receipt = if rows.is_empty() {
                    "No inspections yet. Say ‘start a new inspection’.".into()
                } else {
                    rows.iter()
                        .map(|r| {
                            format!(
                                "{} #{} · {}{} · {} · {}",
                                if inspection_run.as_deref()
                                    == Some(r.get::<String, _>("id").as_str())
                                {
                                    "▶"
                                } else {
                                    "•"
                                },
                                r.get::<i64, _>("run_number"),
                                r.get::<String, _>("title"),
                                r.get::<Option<String>, _>("report_reference")
                                    .map(|v| format!(" · {v}"))
                                    .unwrap_or_default(),
                                r.get::<String, _>("status"),
                                r.get::<String, _>("current_step").replace('_', " ")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                };
            }
            Action::AddTeamMember | Action::RemoveTeamMember | Action::ShowTeam => {
                let id = project
                    .clone()
                    .context("select a project before managing its team")?;
                let lead: Option<i64> =
                    db::query_scalar("SELECT lead_user FROM projects WHERE id=?")
                        .bind(&id)
                        .fetch_one(&mut tx)
                        .await?;
                ensure!(
                    is_superuser || lead == Some(actor_id),
                    "only the project lead or a superuser can change its team"
                );
                if p.action == Action::ShowTeam {
                    let rows = db::query("SELECT u.display_name,u.username,CASE WHEN p.lead_user=u.id THEN 1 ELSE 0 END AS is_lead FROM project_members pm JOIN users u ON u.id=pm.user JOIN projects p ON p.id=pm.project WHERE pm.project=? ORDER BY is_lead DESC,u.display_name")
                        .bind(&id).fetch_all(&mut tx).await?;
                    receipt = rows
                        .iter()
                        .map(|r| {
                            format!(
                                "• {}{} — {}",
                                r.get::<String, _>("display_name"),
                                r.get::<Option<String>, _>("username")
                                    .map(|v| format!(" (@{v})"))
                                    .unwrap_or_default(),
                                if r.get::<i64, _>("is_lead") == 1 {
                                    "lead"
                                } else {
                                    "member"
                                }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                } else {
                    let member = p
                        .member
                        .as_deref()
                        .context("say which @username to add or remove")?
                        .trim()
                        .trim_start_matches('@')
                        .to_lowercase();
                    let target = if let Ok(telegram_id) = member.parse::<i64>() {
                        db::query("SELECT id,display_name,username FROM users WHERE telegram_user_id=? AND status='approved'").bind(telegram_id).fetch_optional(&mut tx).await?
                    } else {
                        db::query("SELECT id,display_name,username FROM users WHERE lower(username)=? AND status='approved'").bind(&member).fetch_optional(&mut tx).await?
                    }.context("that person is not an approved DataDog user")?;
                    let target_id: i64 = target.get("id");
                    if p.action == Action::AddTeamMember {
                        db::query("INSERT OR IGNORE INTO project_members(project,user,added_by,created_at) VALUES(?,?,?,?)").bind(&id).bind(target_id).bind(actor_id).bind(now()).execute(&mut tx).await?;
                        receipt = format!(
                            "Added {} to the project team.",
                            target.get::<String, _>("display_name")
                        );
                    } else {
                        ensure!(
                            lead != Some(target_id),
                            "the project lead cannot be removed from the team"
                        );
                        db::query("DELETE FROM project_members WHERE project=? AND user=?")
                            .bind(&id)
                            .bind(target_id)
                            .execute(&mut tx)
                            .await?;
                        receipt = format!(
                            "Removed {} from the project team.",
                            target.get::<String, _>("display_name")
                        );
                    }
                    changed = true;
                }
            }
            Action::RenameProject | Action::ArchiveProject | Action::RestoreProject => {
                let id = if let Some(loc) = &p.location {
                    db::query_scalar::<String>("SELECT p.id FROM projects p LEFT JOIN aliases a ON a.project=p.id WHERE (p.location_key=? OR (a.chat=? AND a.location_key=?)) AND (? OR p.lead_user=?) LIMIT 1")
                    .bind(normalize(loc)).bind(job.chat).bind(normalize(loc)).bind(is_superuser).bind(actor_id)
                    .fetch_optional(&mut tx)
                    .await?
                } else {
                    project.clone()
                };
                if let Some(id) = id {
                    let lead: Option<i64> =
                        db::query_scalar("SELECT lead_user FROM projects WHERE id=?")
                            .bind(&id)
                            .fetch_one(&mut tx)
                            .await?;
                    ensure!(
                        is_superuser || lead == Some(actor_id),
                        "only the project lead or a superuser can rename or archive it"
                    );
                    if p.action == Action::RenameProject {
                        if let Some(loc) = p
                            .new_location
                            .as_deref()
                            .filter(|v| !normalize(v).is_empty())
                        {
                            let key = normalize(loc);
                            let display_location = title_case_name(loc);
                            ensure!(key.len() <= 200, "location too long");
                            let conflict: Option<String> = db::query_scalar(
                                "SELECT project FROM aliases WHERE chat=? AND location_key=?",
                            )
                            .bind(job.chat)
                            .bind(&key)
                            .fetch_optional(&mut tx)
                            .await?;
                            if conflict.as_deref().is_some_and(|v| v != id) {
                                question=Some("That location already belongs to another project. Please choose a different name.".into());
                            } else {
                                db::query(
                                    "UPDATE projects SET location=?,location_key=? WHERE id=?",
                                )
                                .bind(&display_location)
                                .bind(&key)
                                .bind(&id)
                                .execute(&mut tx)
                                .await?;
                                db::query("INSERT OR IGNORE INTO aliases(chat,location_key,project) VALUES(?,?,?)").bind(job.chat).bind(key).bind(&id).execute(&mut tx).await?;
                                receipt = format!("Renamed project to {display_location}.");
                                changed = true;
                            }
                        } else {
                            question = Some("What should the new location name be?".into());
                        }
                    } else {
                        let archived = i64::from(p.action == Action::ArchiveProject);
                        db::query("UPDATE projects SET archived=? WHERE id=?")
                            .bind(archived)
                            .bind(&id)
                            .execute(&mut tx)
                            .await?;
                        if archived == 1 {
                            db::query(
                                "UPDATE chats SET project=NULL,inspection_run=NULL WHERE project=?",
                            )
                            .bind(&id)
                            .execute(&mut tx)
                            .await?;
                            db::query("DELETE FROM contexts WHERE project=?")
                                .bind(&id)
                                .execute(&mut tx)
                                .await?;
                            project = None;
                            receipt = "Project archived; evidence retained.".into();
                        } else {
                            receipt = "Project restored. Ask me to select it to continue.".into();
                        }
                        changed = true;
                    }
                } else {
                    question = Some("Which existing project location do you mean?".into());
                }
            }
            Action::Clarify => {
                let suggestion = p.question.clone().unwrap_or_else(|| {
                    "Please specify the project, elevation or photo label.".into()
                });
                question = Some(if current.is_none() {
                    format!("Did you mean: {suggestion}\n\n{}", help_text())
                } else {
                    suggestion
                });
            }
            _ => {}
        }
        let needs_assignment = p.action == Action::UpdateEvidence
            || p.action == Action::CreateFinding
            || current.as_ref().is_some_and(|e| e.path.is_some())
            || (p.action == Action::SelectProject
                && target.as_ref().is_some_and(|e| e.project.is_none()));
        let needs_project = needs_assignment
            || p.action == Action::AddElevation
            || p.action == Action::ShowChecklist
            || matches!(
                p.action,
                Action::CreateInspection | Action::SelectInspection | Action::ListInspections
            );
        if needs_project && project.is_none() && question.is_none() {
            question = Some(
                "What is the building name or location? Say ‘new project, Star Vista’.".into(),
            );
        }
        if let Some(ref project) = project {
            if question.is_none() {
                let archived: i64 = db::query_scalar("SELECT archived FROM projects WHERE id=?")
                    .bind(project)
                    .fetch_one(&mut tx)
                    .await?;
                if archived != 0 {
                    question = Some("This project is archived. Restore it to add evidence.".into());
                }
                if question.is_none() {
                    if needs_assignment && inspection_run.is_none() {
                        question =
                            Some("Select or create an inspection before adding evidence.".into());
                    }
                }
                if question.is_none() {
                    for name in &p.elevations {
                        elevation(&mut tx, project, name).await?;
                        changed = true;
                    }
                    if p.action == Action::AddElevation && p.elevations.is_empty() {
                        if let Some(name) = &p.elevation {
                            elevation(&mut tx, project, name).await?;
                            changed = true;
                        } else {
                            question = Some("Which elevations should I add?".into());
                        }
                    }
                    if p.action == Action::AddElevation && changed {
                        receipt = "Elevations added.\n\nNext: send one overview photo per elevation.\nExample label: North elevation overview".into();
                    }
                    if needs_assignment {
                        if let Some(ref e) = target {
                            if e.project.as_deref().is_some_and(|v| v != project) {
                                question=Some("That evidence belongs to a different project. Select its project before correcting it.".into());
                            } else if e
                                .inspection_run
                                .as_deref()
                                .is_some_and(|v| inspection_run.as_deref() != Some(v))
                            {
                                question=Some("That evidence belongs to a different inspection. Select that inspection before correcting it.".into());
                            }
                        }
                        let finding_required =
                            matches!(p.label, Some(Label::ContextPhoto | Label::CloseUp))
                                || p.description.is_some()
                                || p.action == Action::CreateFinding
                                || p.new_finding;
                        let mut elevation_id = if let Some(name) = &p.elevation {
                            Some(elevation(&mut tx, project, name).await?)
                        } else {
                            target.as_ref().and_then(|e| e.elevation)
                        };
                        if let Some(number) = p.finding.as_deref().and_then(|v| parse_id(v, 'F')) {
                            let row = db::query(
                                "SELECT id,elevation FROM findings WHERE project=? AND inspection_run=? AND number=?",
                            )
                            .bind(project)
                            .bind(&inspection_run)
                            .bind(number)
                            .fetch_optional(&mut tx)
                            .await?;
                            if let Some(r) = row {
                                finding = Some(r.get::<i64, _>("id"));
                                let fe = r.get::<i64, _>("elevation");
                                if elevation_id.is_some_and(|e| e != fe) {
                                    question=Some("That finding is on a different elevation. Please confirm the finding ID.".into());
                                }
                                elevation_id = Some(fe);
                            } else {
                                question = Some(format!(
                                    "Finding F{number:03} does not exist in this project. Say ‘new finding’ with its elevation."
                                ));
                            }
                        } else if !p.new_finding && finding_required {
                            finding = target.as_ref().and_then(|e| e.finding);
                            if finding.is_none()
                                && context["active_project"].as_str() == Some(project)
                            {
                                if let Some(number) = context["sender_context"]["finding"]["id"]
                                    .as_str()
                                    .and_then(|v| parse_id(v, 'F'))
                                {
                                    finding = db::query_scalar(
                                        "SELECT id FROM findings WHERE project=? AND inspection_run=? AND number=?",
                                    )
                                    .bind(project)
                                    .bind(&inspection_run)
                                    .bind(number)
                                    .fetch_optional(&mut tx)
                                    .await?;
                                }
                            }
                        }
                        if let Some(fid) = finding {
                            let fe: i64 =
                                db::query_scalar("SELECT elevation FROM findings WHERE id=?")
                                    .bind(fid)
                                    .fetch_one(&mut tx)
                                    .await?;
                            if elevation_id.is_some_and(|e| e != fe) {
                                question=Some("Please specify an existing finding ID, or say ‘new finding’ for this elevation.".into());
                            }
                            elevation_id = Some(fe);
                        }
                        if finding_required && finding.is_none() && question.is_none() {
                            if (p.new_finding || p.action == Action::CreateFinding)
                                && elevation_id.is_some()
                            {
                                let number:i64=db::query_scalar("SELECT COALESCE(MAX(number),0)+1 FROM findings WHERE project=?").bind(project).fetch_one(&mut tx).await?;
                                finding=Some(db::query("INSERT INTO findings(project,inspection_run,number,elevation,level,created_at) VALUES(?,?,?,?,?,?)").bind(project).bind(&inspection_run).bind(number).bind(elevation_id).bind(&p.level).bind(now()).execute(&mut tx).await?.last_insert_rowid());
                                changed = true;
                            } else {
                                question=Some("Which finding is this for? Use an existing F number, or say ‘new finding, East elevation, level 3’.".into());
                            }
                        }
                        if let Some(label) = p.label {
                            if question.is_none()
                                && label == Label::ElevationOverview
                                && elevation_id.is_none()
                            {
                                question = Some("Which elevation does this overview show?".into());
                            }
                            if question.is_none() && (!p.certain || label == Label::Uncertain) {
                                question=Some(p.question.clone().unwrap_or_else(||"Is this a building overview, elevation overview, finding context photo, close-up, or plan?".into()));
                            }
                            if question.is_none() {
                                if let Some(ref e) = target {
                                    ensure!(e.path.is_some(), "photo label requires a saved image");
                                    let source = if job.text.trim().is_empty() {
                                        "vision"
                                    } else {
                                        "caption_or_manual"
                                    };
                                    // A re-label clears the old slot; one photo has one primary assignment.
                                    let is_finding =
                                        matches!(label, Label::ContextPhoto | Label::CloseUp);
                                    let is_elevation =
                                        is_finding || label == Label::ElevationOverview;
                                    db::query("UPDATE evidence SET project=?,inspection_run=?,elevation=?,finding=?,label=?,assignment_source=?,status=? WHERE id=?")
                  .bind(project).bind(&inspection_run).bind(if is_elevation{elevation_id}else{None}).bind(if is_finding{finding}else{None}).bind(label.key()).bind(source).bind(if label==Label::Unrelated{"unrelated"}else{"assigned"}).bind(e.id).execute(&mut tx).await?;
                                    db::query("UPDATE messages SET status='done' WHERE id=? AND status IN ('pending','failed')").bind(e.message).execute(&mut tx).await?;
                                    receipt.push_str(&format!(
                                        "\nE{:04} · {} ✓",
                                        e.id,
                                        label.title()
                                    ));
                                    changed = true;
                                } else {
                                    question = Some(
                                        "Please send or reply to the photo you want to label."
                                            .into(),
                                    );
                                }
                            }
                        } else if target
                            .as_ref()
                            .is_some_and(|e| e.path.is_some() && e.label.is_none())
                            && question.is_none()
                            && p.description.is_none()
                            && p.action != Action::CreateFinding
                        {
                            question=Some("How should I label this photo: building overview, plan, elevation overview, context photo or close-up?".into());
                        }
                        if let Some(ref description) = p.description {
                            if finding.is_some() {
                                ensure!(
                                    !description.trim().is_empty()
                                        && job.text.contains(description),
                                    "description must quote the submitted text"
                                );
                                let fid = finding.context("description requires a finding")?;
                                // Descriptions are independent text evidence even when a caption labels a photo.
                                db::query("INSERT OR IGNORE INTO evidence(message,project,inspection_run,elevation,finding,description,status,assignment_source,created_at,captured_at) VALUES(?,?,?,?,?,?,'assigned','text',?,?)")
                                    .bind(job.id).bind(project).bind(&inspection_run).bind(elevation_id).bind(fid).bind(description).bind(now()).bind(now()).execute(&mut tx).await?;
                                let current_id: i64 =
                                    db::query_scalar("SELECT id FROM evidence WHERE message=?")
                                        .bind(job.id)
                                        .fetch_one(&mut tx)
                                        .await?;
                                db::query("UPDATE evidence SET description=? WHERE id=?")
                                    .bind(description)
                                    .bind(current_id)
                                    .execute(&mut tx)
                                    .await?;
                                db::query("INSERT INTO descriptions(message,finding,evidence,text,created_at) VALUES(?,?,?,?,?) ON CONFLICT(message,finding) DO UPDATE SET text=excluded.text")
                                    .bind(job.id).bind(fid).bind(current_id).bind(description).bind(now()).execute(&mut tx).await?;
                                receipt.push_str("\nDescription recorded.");
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
        if needs_assignment {
            if let Some(ref e) = target {
                if e.project.is_none() {
                    db::query("UPDATE evidence SET project=?,inspection_run=? WHERE id=?")
                        .bind(&project)
                        .bind(&inspection_run)
                        .bind(e.id)
                        .execute(&mut tx)
                        .await?;
                }
            }
        }
        let eid = target.as_ref().map(|e| e.id);
        if let Some(evidence_id) = eid {
            if let Some(zone) = p.zone.as_deref().filter(|zone| !zone.trim().is_empty()) {
                db::query("UPDATE evidence SET zone=? WHERE id=?")
                    .bind(zone.trim())
                    .bind(evidence_id)
                    .execute(&mut tx)
                    .await?;
                changed = true;
            }
            if !p.observations.is_empty() {
                db::query("UPDATE evidence SET visual_observations=? WHERE id=?")
                    .bind(serde_json::to_string(&p.observations)?)
                    .bind(evidence_id)
                    .execute(&mut tx)
                    .await?;
                changed = true;
                let summary = p
                    .observations
                    .iter()
                    .take(2)
                    .map(|item| {
                        let checks = if item.verification.is_empty() {
                            "CP/FI review".to_owned()
                        } else {
                            format!("check: {}", item.verification.join(", "))
                        };
                        format!("• {} ({checks})", item.note)
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                receipt.push_str(&format!("\nAI — verify on site:\n{summary}"));
            }
            if let Some(note) = p
                .field_note
                .as_deref()
                .filter(|note| job.text.contains(*note))
            {
                db::query("UPDATE evidence SET field_note=CASE WHEN field_note IS NULL OR field_note='' THEN ? ELSE field_note || char(10) || ? END WHERE id=?")
                    .bind(note)
                    .bind(note)
                    .bind(evidence_id)
                    .execute(&mut tx)
                    .await?;
                changed = true;
            }
        }
        if needs_assignment || question.is_some() || finding.is_some() {
            db::query("INSERT INTO contexts(chat,topic,sender,project,finding,evidence,prompt) VALUES(?,?,?,?,?,?,?) ON CONFLICT(chat,topic,sender) DO UPDATE SET project=excluded.project,finding=COALESCE(excluded.finding,contexts.finding),evidence=excluded.evidence,prompt=excluded.prompt")
       .bind(job.chat).bind(job.topic).bind(&job.sender).bind(&project).bind(finding).bind(if question.is_some(){eid}else{None}).bind(&question).execute(&mut tx).await?;
        }
        if changed {
            if let Some(run) = inspection_run.as_deref() {
                sync_inspection_progress(&mut tx, project.as_deref(), run).await?;
                auto_report=db::query_scalar::<i64>("SELECT EXISTS(SELECT 1 FROM inspection_runs r WHERE r.id=? AND r.status='reviewing' AND NOT EXISTS(SELECT 1 FROM report_drafts d WHERE d.inspection_run=r.id))")
                    .bind(run).fetch_one(&mut tx).await?==1;
                if auto_report {
                    receipt.push_str("\n\nChecklist complete. Generating the DOCX draft.");
                }
            }
        }
        if changed {
            db::query("INSERT INTO audit(message,evidence,action,before_json,after_json,created_at) VALUES(?,?,?,?,?,?)").bind(job.id).bind(eid).bind(format!("{:?}",p.action)).bind(context.to_string()).bind(serde_json::to_string(p)?).bind(now()).execute(&mut tx).await?;
        }
        if let Some(q) = question {
            if !receipt.trim().is_empty() {
                receipt.push('\n');
            }
            if eid.is_some() {
                receipt.push_str("Needs: ");
            }
            receipt.push_str(&q);
        }
        if changed && receipt.trim().is_empty() {
            receipt = "Checklist updated.".into();
        }
        if !receipt.trim().is_empty() {
            queue(&mut tx, job, "reply", receipt.trim(), eid).await?;
        }
        if p.action == Action::ShowChecklist || p.action == Action::SelectProject {
            queue(&mut tx, job, "checklist_show", "", None).await?;
        } else if changed {
            queue(&mut tx, job, "checklist", "", None).await?;
        }
        db::query("UPDATE messages SET status='done',error=NULL WHERE id=?")
            .bind(job.id)
            .execute(&mut tx)
            .await?;
        tx.commit().await?;
        if auto_report {
            if let Some(run) = inspection_run.as_deref() {
                if let Err(error) = self.generate_report(job, context).await {
                    tracing::warn!(inspection_run=run,error=%error,"Automatic report generation deferred");
                }
            }
        }
        if let Some(fid) = finding {
            if self.prompt_finding(job, fid).await.is_err() {
                tracing::warn!("Could not queue follow-up prompt");
            }
        }
        Ok(())
    }

    pub async fn generate_report(&self, job: &Job, context: &Value) -> Result<()> {
        let project_id = context["active_project"]
            .as_str()
            .context("select a project before generating its report")?;
        let inspection_run = context["active_inspection"]["id"]
            .as_str()
            .context("select or create an inspection before generating its report")?;
        let actor_telegram_id = job
            .sender
            .strip_prefix("user:")
            .and_then(|v| v.parse::<i64>().ok())
            .context("invalid message sender")?;
        let project = db::query("SELECT p.id,p.location,u.id AS actor_id FROM projects p JOIN users u ON u.telegram_user_id=? WHERE p.id=? AND p.archived=0 AND (u.role='superuser' OR p.lead_user=u.id OR EXISTS(SELECT 1 FROM project_members pm WHERE pm.project=p.id AND pm.user=u.id))")
            .bind(actor_telegram_id)
            .bind(project_id)
            .fetch_one(&self.db)
            .await?;
        let location: String = project.get("location");
        let actor_id: i64 = project.get("actor_id");
        let run=db::query("SELECT run_number,title,report_reference,status,current_step,inspection_started_at,inspection_ended_at,notice_reference,notice_date,report_due_at,metadata FROM inspection_runs WHERE id=? AND project=?")
            .bind(inspection_run).bind(project_id).fetch_one(&self.db).await?;
        let version: i64 = db::query_scalar(
            "SELECT COALESCE(MAX(version),0)+1 FROM report_drafts WHERE inspection_run=?",
        )
        .bind(inspection_run)
        .fetch_one(&self.db)
        .await?;
        let mut slug: String = location
            .to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(32)
            .collect();
        if slug.is_empty() {
            slug = "building".into();
        }
        let report_dir = self.data.join("reports");
        tokio::fs::create_dir_all(&report_dir).await?;
        let output_path = report_dir.join(format!(
            "pfi-draft-{slug}-inspection-{}-v{version}.docx",
            run.get::<i64, _>("run_number")
        ));

        let elevation_rows =
            db::query("SELECT id,name FROM elevations WHERE project=? ORDER BY id")
                .bind(project_id)
                .fetch_all(&self.db)
                .await?;
        let mut elevations = Vec::new();
        for elevation in elevation_rows {
            let id: i64 = elevation.get("id");
            let name: String = elevation.get("name");
            let finding_rows = db::query(
                "SELECT number,level FROM findings WHERE project=? AND inspection_run=? AND elevation=? ORDER BY number",
            )
            .bind(project_id)
            .bind(inspection_run)
            .bind(id)
            .fetch_all(&self.db)
            .await?;
            let findings: Vec<Value> = finding_rows
                .iter()
                .map(|finding| json!({"number":finding.get::<i64,_>("number"),"level":finding.get::<Option<String>,_>("level")}))
                .collect();
            elevations.push(json!({"id":id,"name":name,"findings":findings}));
        }

        let evidence_rows = db::query("SELECT e.id,e.elevation,e.finding,e.label,e.status,e.description,e.path,e.preview_path,e.filename,e.mime,e.width,e.height,e.created_at,e.captured_at,e.capture_metadata,e.visual_observations,e.field_note,e.zone,m.sent_at,el.name AS elevation_name,f.number AS finding_number,f.level AS finding_level FROM evidence e JOIN messages m ON m.id=e.message LEFT JOIN elevations el ON el.id=e.elevation LEFT JOIN findings f ON f.id=e.finding WHERE e.project=? AND e.inspection_run=? ORDER BY e.id")
            .bind(project_id)
            .bind(inspection_run)
            .fetch_all(&self.db)
            .await?;
        let mut evidence = Vec::new();
        for row in evidence_rows {
            let observations: Value = row
                .get::<Option<String>, _>("visual_observations")
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or_else(|| json!([]));
            evidence.push(json!({
                "id":row.get::<i64,_>("id"),
                "elevation":row.get::<Option<String>,_>("elevation_name"),
                "finding":row.get::<Option<i64>,_>("finding_number").map(|n|format!("F{n:03}")),
                "level":row.get::<Option<String>,_>("finding_level"),
                "label":row.get::<Option<String>,_>("label"),
                "status":row.get::<String,_>("status"),
                "description":row.get::<Option<String>,_>("description"),
                "field_note":row.get::<Option<String>,_>("field_note"),
                "zone":row.get::<Option<String>,_>("zone"),
                "source_path":row.get::<Option<String>,_>("path"),
                "preview_path":row.get::<Option<String>,_>("preview_path"),
                "filename":row.get::<Option<String>,_>("filename"),
                "mime":row.get::<Option<String>,_>("mime"),
                "width":row.get::<Option<i64>,_>("width"),
                "height":row.get::<Option<i64>,_>("height"),
                "sent_at":row.get::<i64,_>("sent_at"),
                "captured_at":row.get::<Option<i64>,_>("captured_at"),
                "capture_metadata":row.get::<String,_>("capture_metadata"),
                "observations":observations
            }));
        }
        let description_rows = db::query("SELECT f.number AS finding_number,d.text FROM descriptions d JOIN findings f ON f.id=d.finding WHERE f.project=? AND f.inspection_run=? AND d.text<>'' ORDER BY f.number,d.created_at")
            .bind(project_id)
            .bind(inspection_run)
            .fetch_all(&self.db)
            .await?;
        let descriptions: Vec<Value> = description_rows
            .iter()
            .map(|row| {
                json!({
                    "finding":format!("F{:03}", row.get::<i64,_>("finding_number")),
                    "text":row.get::<String,_>("text")
                })
            })
            .collect();
        let prior_reports=db::query("SELECT r.run_number,r.title,r.report_reference,r.status,r.inspection_started_at,r.inspection_ended_at,MAX(d.version) AS latest_version FROM inspection_runs r LEFT JOIN report_drafts d ON d.inspection_run=r.id WHERE r.project=? AND r.id<>? GROUP BY r.id ORDER BY r.run_number DESC LIMIT 10")
            .bind(project_id).bind(inspection_run).fetch_all(&self.db).await?.iter().map(|r|json!({"run_number":r.get::<i64,_>("run_number"),"title":r.get::<String,_>("title"),"report_reference":r.get::<Option<String>,_>("report_reference"),"status":r.get::<String,_>("status"),"inspection_started_at":r.get::<Option<i64>,_>("inspection_started_at"),"inspection_ended_at":r.get::<Option<i64>,_>("inspection_ended_at"),"latest_version":r.get::<Option<i64>,_>("latest_version")})).collect::<Vec<_>>();
        let snapshot = json!({
            "project":{"id":project_id,"location":location},
            "inspection":{"id":inspection_run,"run_number":run.get::<i64,_>("run_number"),"title":run.get::<String,_>("title"),"report_reference":run.get::<Option<String>,_>("report_reference"),"status":run.get::<String,_>("status"),"current_step":run.get::<String,_>("current_step"),"inspection_started_at":run.get::<Option<i64>,_>("inspection_started_at"),"inspection_ended_at":run.get::<Option<i64>,_>("inspection_ended_at"),"notice_reference":run.get::<Option<String>,_>("notice_reference"),"notice_date":run.get::<Option<i64>,_>("notice_date"),"report_due_at":run.get::<Option<i64>,_>("report_due_at"),"version":version},
            "prior_reports":prior_reports,
            "elevations":elevations,
            "evidence":evidence,
            "descriptions":descriptions,
            "generated_at":now(),
            "notice":"Working draft compiled from records in DataDog. Missing fields require completion and review by the appointed Competent Person."
        });
        let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let script = env::var_os("REPORT_SCRIPT")
            .map(PathBuf::from)
            .unwrap_or_else(|| crate_dir.join("report.py"));
        let mut command = if let Ok(python) = env::var("REPORT_PYTHON") {
            let mut command = Command::new(python);
            command.arg(script);
            command
        } else {
            let mut command = Command::new("uv");
            command
                .arg("run")
                .arg("--project")
                .arg(crate_dir)
                .arg("python")
                .arg(script);
            command
        };
        let mut child = command
            .arg("--output")
            .arg(&output_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("could not start the DOCX report generator")?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(&serde_json::to_vec(&snapshot)?)
                .await
                .context("could not send project data to the report generator")?;
        }
        let result = child.wait_with_output().await?;
        ensure!(
            result.status.success(),
            "DOCX report generation failed: {}",
            String::from_utf8_lossy(&result.stderr)
                .lines()
                .last()
                .unwrap_or("unknown error")
        );
        ensure!(output_path.is_file(), "DOCX report was not created");
        let report_bytes = tokio::fs::read(&output_path).await?;
        let sha256 = format!("{:x}", Sha256::digest(&report_bytes));
        let mut tx = self.db.begin().await?;
        db::query("INSERT INTO report_drafts(id,inspection_run,version,status,template_version,snapshot_json,path,sha256,generated_by,generated_at) VALUES(?,?,?,'draft','pfi-v1',?,?,?,?,?)")
            .bind(uuid::Uuid::new_v4().to_string()).bind(inspection_run).bind(version).bind(snapshot.to_string()).bind(output_path.to_string_lossy().as_ref()).bind(sha256).bind(actor_id).bind(now()).execute(&mut tx).await?;
        db::query("UPDATE inspection_runs SET status='cp_review',current_step='cp_review',updated_at=? WHERE id=?").bind(now()).bind(inspection_run).execute(&mut tx).await?;
        db::query("UPDATE inspection_steps SET status='complete',completed_at=COALESCE(completed_at,?) WHERE inspection_run=? AND step_key IN ('findings_review','report_drafting')").bind(now()).bind(inspection_run).execute(&mut tx).await?;
        db::query("UPDATE inspection_steps SET status='in_progress',started_at=COALESCE(started_at,?) WHERE inspection_run=? AND step_key='cp_review'").bind(now()).bind(inspection_run).execute(&mut tx).await?;
        db::query("INSERT INTO outbox(chat,topic,reply_to,kind,text,path,source_message) VALUES(?,?,?,'document',?,?,?)")
            .bind(job.chat)
            .bind(job.topic)
            .bind(job.telegram_id)
            .bind(format!("PFI draft v{version} · {location} · inspection #{}. CP review and certification required.",run.get::<i64,_>("run_number")))
            .bind(output_path.to_string_lossy().as_ref())
            .bind(job.id)
            .execute(&mut tx)
            .await?;
        db::query("UPDATE messages SET status='done',error=NULL WHERE id=?")
            .bind(job.id)
            .execute(&mut tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
    async fn prompt_finding(&self, job: &Job, fid: i64) -> Result<()> {
        let r=db::query("SELECT number, EXISTS(SELECT 1 FROM evidence WHERE finding=f.id AND label='context_photo' AND path IS NOT NULL) AS context_done, EXISTS(SELECT 1 FROM evidence WHERE finding=f.id AND label='close_up' AND path IS NOT NULL) AS close_done, EXISTS(SELECT 1 FROM descriptions WHERE finding=f.id AND text<>'') AS description_done FROM findings f WHERE id=?").bind(fid).fetch_one(&self.db).await?;
        let missing = if r.get::<i64, _>("context_done") == 0 {
            Some("a context photo showing where the finding is")
        } else if r.get::<i64, _>("close_done") == 0 {
            Some("a close-up photo")
        } else if r.get::<i64, _>("description_done") == 0 {
            Some("a text description of the observed condition")
        } else {
            None
        };
        if let Some(missing) = missing {
            let previous: Option<i64> = db::query_scalar::<Option<i64>>(
                "SELECT prompted_finding FROM contexts WHERE chat=? AND topic=? AND sender=?",
            )
            .bind(job.chat)
            .bind(job.topic)
            .bind(&job.sender)
            .fetch_optional(&self.db)
            .await?
            .flatten();
            if previous != Some(fid) {
                self.enqueue(
                    job,
                    "reply",
                    &format!(
                        "For F{:03}, please send {missing}.",
                        r.get::<i64, _>("number")
                    ),
                    None,
                    None,
                )
                .await?;
                db::query(
                    "UPDATE contexts SET prompted_finding=? WHERE chat=? AND topic=? AND sender=?",
                )
                .bind(fid)
                .bind(job.chat)
                .bind(job.topic)
                .bind(&job.sender)
                .execute(&self.db)
                .await?;
            }
        }
        Ok(())
    }
    pub async fn checklist(
        &self,
        project: Option<&str>,
        inspection_run: Option<&str>,
    ) -> Result<String> {
        let Some(project) = project else {
            return Ok("No active project. Say ‘new project, Star Vista’.".into());
        };
        let Some(run) = inspection_run else {
            return Ok("No active inspection. Say ‘start a new inspection’.".into());
        };
        let row=db::query("SELECT p.location,r.run_number,r.title,r.report_reference,r.status,r.current_step FROM projects p JOIN inspection_runs r ON r.project=p.id WHERE p.id=? AND r.id=?")
            .bind(project).bind(run).fetch_one(&self.db).await?;
        let location: String = row.get("location");
        let reference = row
            .get::<Option<String>, _>("report_reference")
            .map(|v| format!(" · {v}"))
            .unwrap_or_default();
        let mut output = format!(
            "{location}\nInspection #{} · {}{reference}\nStatus: {} · Current step: {}\n",
            row.get::<i64, _>("run_number"),
            row.get::<String, _>("title"),
            row.get::<String, _>("status").replace('_', " "),
            row.get::<String, _>("current_step").replace('_', " ")
        );

        let steps = db::query(
            "SELECT title,status FROM inspection_steps WHERE inspection_run=? ORDER BY ordinal",
        )
        .bind(run)
        .fetch_all(&self.db)
        .await?;
        output.push_str("\nWorkflow\n");
        for step in steps {
            let status: String = step.get("status");
            let marker = match status.as_str() {
                "complete" => "☑",
                "in_progress" => "▶",
                "blocked" => "⚠",
                "skipped" => "–",
                _ => "☐",
            };
            output.push_str(&format!("{marker} {}\n", step.get::<String, _>("title")));
        }
        output.push_str("\nEvidence\n");
        let mut done = 0_i64;
        let mut total = 2_i64;
        for (key, title) in [
            ("building_overview", "Building overview"),
            ("plan", "Plan drawing"),
        ] {
            let count:i64=db::query_scalar("SELECT count(*) FROM evidence WHERE project=? AND inspection_run=? AND label=? AND path IS NOT NULL").bind(project).bind(run).bind(key).fetch_one(&self.db).await?;
            if count > 0 {
                done += 1;
            }
            output.push_str(&format!("{} {title}\n", if count > 0 { "☑" } else { "☐" }));
        }
        let elevations = db::query("SELECT id,name FROM elevations WHERE project=? ORDER BY id")
            .bind(project)
            .fetch_all(&self.db)
            .await?;
        if elevations.is_empty() {
            output.push_str("\nAdd elevations, e.g. ‘track North, South, East and West’.\n");
        }
        for elevation in elevations {
            let elevation_id: i64 = elevation.get("id");
            total += 1;
            let overview:i64=db::query_scalar("SELECT count(*) FROM evidence WHERE project=? AND inspection_run=? AND elevation=? AND label='elevation_overview' AND path IS NOT NULL").bind(project).bind(run).bind(elevation_id).fetch_one(&self.db).await?;
            if overview > 0 {
                done += 1;
            }
            output.push_str(&format!(
                "\n{}\n{} Elevation overview\n",
                elevation.get::<String, _>("name"),
                if overview > 0 { "☑" } else { "☐" }
            ));
            let findings=db::query("SELECT id,number,level FROM findings WHERE project=? AND inspection_run=? AND elevation=? ORDER BY number").bind(project).bind(run).bind(elevation_id).fetch_all(&self.db).await?;
            for finding in findings {
                let finding_id: i64 = finding.get("id");
                output.push_str(&format!(
                    "F{:03}{}\n",
                    finding.get::<i64, _>("number"),
                    finding
                        .get::<Option<String>, _>("level")
                        .map(|v| format!(" — {v}"))
                        .unwrap_or_default()
                ));
                for (key, title) in [
                    ("context_photo", "Context photo"),
                    ("close_up", "Close-up photo"),
                    ("description", "Description"),
                ] {
                    total += 1;
                    let count: i64 = if key == "description" {
                        db::query_scalar(
                            "SELECT count(*) FROM descriptions WHERE finding=? AND text<>''",
                        )
                        .bind(finding_id)
                        .fetch_one(&self.db)
                        .await?
                    } else {
                        db::query_scalar("SELECT count(*) FROM evidence WHERE project=? AND inspection_run=? AND finding=? AND label=? AND path IS NOT NULL").bind(project).bind(run).bind(finding_id).bind(key).fetch_one(&self.db).await?
                    };
                    if count > 0 {
                        done += 1;
                    }
                    output.push_str(&format!(
                        "  {} {title}\n",
                        if count > 0 { "☑" } else { "☐" }
                    ));
                }
            }
        }
        let percent = if total == 0 { 0 } else { done * 100 / total };
        let unassigned:i64=db::query_scalar("SELECT count(*) FROM evidence WHERE project=? AND inspection_run=? AND status='unassigned' AND path IS NOT NULL").bind(project).bind(run).fetch_one(&self.db).await?;
        output.push_str(&format!(
            "\nProgress: {done}/{total} required items ({percent}%)"
        ));
        if unassigned > 0 {
            output.push_str(&format!("\n{unassigned} saved photo(s) awaiting labels."));
        }
        output.push_str(
            "\n\nTicks indicate saved evidence. AI observations require inspector/CP review.",
        );
        Ok(output)
    }
    pub async fn next_outgoing(&self) -> Result<Option<Outgoing>> {
        Ok(db::query_as("SELECT id,chat,topic,reply_to,kind,text,path,evidence,attempts,source_message FROM outbox WHERE delivered_at IS NULL AND next_attempt<=? ORDER BY id LIMIT 1").bind(now()).fetch_optional(&self.db).await?)
    }
}
async fn create_inspection_run(
    tx: &mut Transaction,
    project: &str,
    actor_id: i64,
    title: Option<&str>,
    reference: Option<&str>,
) -> Result<String> {
    let run_number: i64 = db::query_scalar(
        "SELECT COALESCE(MAX(run_number),0)+1 FROM inspection_runs WHERE project=?",
    )
    .bind(project)
    .fetch_one(&mut *tx)
    .await?;
    let id = uuid::Uuid::new_v4().to_string();
    let title = title
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Inspection {run_number}"));
    let reference = reference
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("R{run_number:03}"));
    db::query("INSERT INTO inspection_runs(id,project,run_number,title,report_reference,status,current_step,inspection_started_at,created_by,created_at,updated_at) VALUES(?,?,?,?,?,'collecting','evidence_collection',?,?,?,?)")
        .bind(&id).bind(project).bind(run_number).bind(title).bind(reference).bind(now()).bind(actor_id).bind(now()).bind(now()).execute(&mut *tx).await?;
    for (key, ordinal, title, status) in [
        ("project_setup", 1, "Project setup", "complete"),
        ("document_review", 2, "Document review", "pending"),
        (
            "evidence_collection",
            3,
            "Visual evidence collection",
            "in_progress",
        ),
        (
            "close_range_inspection",
            4,
            "Close-range inspection",
            "pending",
        ),
        ("findings_review", 5, "Findings review", "pending"),
        ("report_drafting", 6, "Report drafting", "pending"),
        ("cp_review", 7, "CP review and certification", "pending"),
    ] {
        db::query("INSERT INTO inspection_steps(inspection_run,step_key,ordinal,title,status,started_at) VALUES(?,?,?,?,?,?)")
            .bind(&id).bind(key).bind(ordinal).bind(title).bind(status).bind(if status=="in_progress"{Some(now())}else{None}).execute(&mut *tx).await?;
    }
    Ok(id)
}
async fn sync_inspection_progress(
    tx: &mut Transaction,
    project: Option<&str>,
    run: &str,
) -> Result<()> {
    let Some(project) = project else {
        return Ok(());
    };
    let status: String = db::query_scalar("SELECT status FROM inspection_runs WHERE id=?")
        .bind(run)
        .fetch_one(&mut *tx)
        .await?;
    if !matches!(status.as_str(), "collecting" | "reviewing") {
        db::query("UPDATE inspection_runs SET updated_at=? WHERE id=?")
            .bind(now())
            .bind(run)
            .execute(&mut *tx)
            .await?;
        return Ok(());
    }
    let elevations: i64 = db::query_scalar("SELECT count(*) FROM elevations WHERE project=?")
        .bind(project)
        .fetch_one(&mut *tx)
        .await?;
    let required: i64 = 2
        + elevations
        + db::query_scalar::<i64>(
            "SELECT count(*)*3 FROM findings WHERE project=? AND inspection_run=?",
        )
        .bind(project)
        .bind(run)
        .fetch_one(&mut *tx)
        .await?;
    let mut complete = 0_i64;
    for label in ["building_overview", "plan"] {
        let count:i64=db::query_scalar("SELECT count(*) FROM evidence WHERE project=? AND inspection_run=? AND label=? AND path IS NOT NULL").bind(project).bind(run).bind(label).fetch_one(&mut *tx).await?;
        complete += if count > 0 { 1 } else { 0 };
    }
    complete+=db::query_scalar::<i64>("SELECT count(*) FROM elevations el WHERE el.project=? AND EXISTS(SELECT 1 FROM evidence e WHERE e.project=? AND e.inspection_run=? AND e.elevation=el.id AND e.label='elevation_overview' AND e.path IS NOT NULL)").bind(project).bind(project).bind(run).fetch_one(&mut *tx).await?;
    let findings = db::query("SELECT id FROM findings WHERE project=? AND inspection_run=?")
        .bind(project)
        .bind(run)
        .fetch_all(&mut *tx)
        .await?;
    for finding in findings {
        let id: i64 = finding.get("id");
        for label in ["context_photo", "close_up"] {
            let count:i64=db::query_scalar("SELECT count(*) FROM evidence WHERE inspection_run=? AND finding=? AND label=? AND path IS NOT NULL").bind(run).bind(id).bind(label).fetch_one(&mut *tx).await?;
            complete += if count > 0 { 1 } else { 0 };
        }
        let count: i64 =
            db::query_scalar("SELECT count(*) FROM descriptions WHERE finding=? AND text<>''")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        complete += if count > 0 { 1 } else { 0 };
    }
    let evidence_complete = elevations > 0 && complete >= required;
    let (run_status, current_step) = if evidence_complete {
        ("reviewing", "findings_review")
    } else {
        ("collecting", "evidence_collection")
    };
    db::query("UPDATE inspection_runs SET status=?,current_step=?,updated_at=? WHERE id=?")
        .bind(run_status)
        .bind(current_step)
        .bind(now())
        .bind(run)
        .execute(&mut *tx)
        .await?;
    db::query("UPDATE inspection_steps SET status=?,completed_at=? WHERE inspection_run=? AND step_key='evidence_collection'").bind(if evidence_complete{"complete"}else{"in_progress"}).bind(if evidence_complete{Some(now())}else{None}).bind(run).execute(&mut *tx).await?;
    db::query("UPDATE inspection_steps SET status=?,started_at=CASE WHEN ? THEN COALESCE(started_at,?) ELSE started_at END WHERE inspection_run=? AND step_key='findings_review'").bind(if evidence_complete{"in_progress"}else{"pending"}).bind(evidence_complete).bind(now()).bind(run).execute(&mut *tx).await?;
    Ok(())
}
async fn elevation(tx: &mut Transaction, project: &str, name: &str) -> Result<i64> {
    let key = normalize(name);
    ensure!(!key.is_empty() && key.len() <= 100, "invalid elevation");
    db::query("INSERT OR IGNORE INTO elevations(project,name,name_key) VALUES(?,?,?)")
        .bind(project)
        .bind(name.trim())
        .bind(key.clone())
        .execute(&mut *tx)
        .await?;
    Ok(
        db::query_scalar("SELECT id FROM elevations WHERE project=? AND name_key=?")
            .bind(project)
            .bind(key)
            .fetch_one(&mut *tx)
            .await?,
    )
}
async fn queue(
    tx: &mut Transaction,
    job: &Job,
    kind: &str,
    text: &str,
    evidence: Option<i64>,
) -> Result<()> {
    db::query("INSERT INTO outbox(chat,topic,reply_to,kind,text,evidence,source_message) VALUES(?,?,?,?,?,?,?)")
        .bind(job.chat)
        .bind(job.topic)
        .bind(job.telegram_id)
        .bind(kind)
        .bind(text)
        .bind(evidence)
        .bind(job.id)
        .execute(&mut *tx)
        .await?;
    if matches!(kind, "checklist" | "checklist_show") {
        db::query("UPDATE outbox SET next_attempt=? WHERE id=last_insert_rowid()")
            .bind(now() + 2)
            .execute(&mut *tx)
            .await?;
    }
    Ok(())
}
pub fn parse_id(s: &str, prefix: char) -> Option<i64> {
    let s = s.trim();
    let rest = s
        .strip_prefix(prefix)
        .or_else(|| s.strip_prefix(prefix.to_ascii_lowercase()))?;
    let id = rest.parse::<i64>().ok()?;
    if id > 0 { Some(id) } else { None }
}
