use crate::deepseek::{Action, Label, Proposal};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sqlx::{
    Row, Sqlite, SqlitePool, Transaction,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

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
#[derive(Clone)]
pub struct Store {
    pub pool: SqlitePool,
    pub data: PathBuf,
}
#[derive(sqlx::FromRow, Clone)]
pub struct Job {
    pub id: i64,
    pub chat: i64,
    pub topic: i64,
    pub telegram_id: i64,
    pub sender: String,
    pub text: String,
    pub raw: String,
    pub project: Option<String>,
    pub attempts: i64,
    pub analysis: Option<String>,
}
#[derive(sqlx::FromRow, Clone)]
pub struct Evidence {
    pub id: i64,
    pub message: i64,
    pub project: Option<String>,
    pub elevation: Option<i64>,
    pub finding: Option<i64>,
    pub label: Option<String>,
    pub description: Option<String>,
    pub path: Option<String>,
    pub preview_path: Option<String>,
    pub yolo_attempts: i64,
}
#[derive(sqlx::FromRow)]
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
}
impl Store {
    pub async fn open(data: &Path) -> Result<Self> {
        tokio::fs::create_dir_all(data.join("evidence")).await?;
        let options = SqliteConnectOptions::new()
            .filename(data.join("telebot.sqlite"))
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(std::time::Duration::from_secs(10));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self {
            pool,
            data: data.to_owned(),
        })
    }
    pub async fn ingest(&self, msg: &teloxide::types::Message) -> Result<()> {
        let raw = serde_json::to_string(msg)?;
        let topic = msg.thread_id.map(|v| v.0.0 as i64).unwrap_or(0);
        let sender = if let Some(user) = &msg.from {
            format!("user:{}", user.id.0)
        } else {
            format!(
                "chat:{}",
                msg.sender_chat
                    .as_ref()
                    .map(|c| c.id.0)
                    .unwrap_or(msg.chat.id.0)
            )
        };
        sqlx::query("INSERT OR IGNORE INTO messages(chat,topic,telegram_id,sender,sent_at,received_at,raw,text,status) VALUES(?,?,?,?,?,?,?,?,?)")
      .bind(msg.chat.id.0).bind(topic).bind(msg.id.0).bind(sender).bind(msg.date.timestamp()).bind(now()).bind(raw).bind(msg.text().or_else(||msg.caption()).unwrap_or("")).bind(if crate::worker::image_file(msg).is_some(){"receiving"}else{"pending"}).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn next_job(&self) -> Result<Option<Job>> {
        Ok(sqlx::query_as("SELECT id,chat,topic,telegram_id,sender,text,raw,project,attempts,analysis FROM messages WHERE status='pending' AND next_attempt<=? AND NOT EXISTS(SELECT 1 FROM messages earlier WHERE earlier.id<messages.id AND earlier.chat=messages.chat AND earlier.topic=messages.topic AND earlier.status='receiving') ORDER BY id LIMIT 1").bind(now()).fetch_optional(&self.pool).await?)
    }
    pub async fn evidence(&self, id: i64) -> Result<Option<Evidence>> {
        Ok(sqlx::query_as("SELECT id,message,project,elevation,finding,label,description,path,preview_path,yolo_attempts FROM evidence WHERE id=?").bind(id).fetch_optional(&self.pool).await?)
    }
    pub async fn current_evidence(&self, job: &Job) -> Result<Option<Evidence>> {
        let id: Option<i64> = sqlx::query_scalar("SELECT id FROM evidence WHERE message=?")
            .bind(job.id)
            .fetch_optional(&self.pool)
            .await?;
        if let Some(id) = id {
            self.evidence(id).await
        } else {
            Ok(None)
        }
    }
    pub async fn reserve_evidence(&self, job: &Job) -> Result<i64> {
        sqlx::query("INSERT OR IGNORE INTO evidence(message,created_at) VALUES(?,?)")
            .bind(job.id)
            .bind(now())
            .execute(&self.pool)
            .await?;
        Ok(
            sqlx::query_scalar("SELECT id FROM evidence WHERE message=?")
                .bind(job.id)
                .fetch_one(&self.pool)
                .await?,
        )
    }
    pub async fn active_project(&self, job: &Job) -> Result<Option<String>> {
        if job.project.is_some() {
            return Ok(job.project.clone());
        }
        Ok(sqlx::query_scalar::<_, Option<String>>(
            "SELECT project FROM chats WHERE chat=? AND topic=?",
        )
        .bind(job.chat)
        .bind(job.topic)
        .fetch_optional(&self.pool)
        .await?
        .flatten())
    }
    pub async fn context(&self, job: &Job) -> Result<Value> {
        let project = self.active_project(job).await?;
        // Freeze a job's project before inference so retries cannot migrate evidence.
        if let Some(p) = &project {
            sqlx::query("UPDATE messages SET project=? WHERE id=? AND project IS NULL")
                .bind(p)
                .bind(job.id)
                .execute(&self.pool)
                .await?;
        }
        let projects=sqlx::query("SELECT id,location,archived FROM projects WHERE owner_chat=? ORDER BY created_at DESC LIMIT 100").bind(job.chat).fetch_all(&self.pool).await?;
        let projects:Vec<Value>=projects.iter().map(|r|json!({"id":r.get::<String,_>("id"),"location":r.get::<String,_>("location"),"archived":r.get::<i64,_>("archived")==1})).collect();
        let context=sqlx::query("SELECT project,finding,evidence,prompt FROM contexts WHERE chat=? AND topic=? AND sender=?").bind(job.chat).bind(job.topic).bind(&job.sender).fetch_optional(&self.pool).await?;
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
            let id:Option<i64>=sqlx::query_scalar("SELECT e.id FROM evidence e JOIN messages m ON m.id=e.message WHERE m.chat=? AND m.topic=? AND m.telegram_id=? UNION ALL SELECT evidence FROM outbox WHERE chat=? AND topic=? AND telegram_message=? AND evidence IS NOT NULL LIMIT 1")
      .bind(job.chat).bind(job.topic).bind(reply_id).bind(job.chat).bind(job.topic).bind(reply_id).fetch_optional(&self.pool).await?;
            if let Some(id) = id {
                reply = self.evidence_json(id).await?;
            }
        }
        let current = if let Some(e) = self.current_evidence(job).await? {
            self.evidence_json(e.id).await?
        } else {
            Value::Null
        };
        let recent=sqlx::query("SELECT text FROM messages WHERE chat=? AND topic=? AND sender=? AND id<? AND text<>'' ORDER BY id DESC LIMIT 4").bind(job.chat).bind(job.topic).bind(&job.sender).bind(job.id).fetch_all(&self.pool).await?;
        let recent: Vec<String> = recent.iter().map(|r| r.get("text")).collect();
        Ok(
            json!({"active_project":project,"projects":projects,"sender_context":sender_context,"pending_evidence":pending,"reply_evidence":reply,"current_evidence":current,"checklist":self.checklist(project.as_deref()).await?,"recent_sender_messages":recent,"reply_text":raw["reply_to_message"]["text"].as_str().or_else(||raw["reply_to_message"]["caption"].as_str())}),
        )
    }
    async fn finding_json(&self, id: Option<i64>) -> Result<Value> {
        if let Some(id) = id {
            if let Some(r)=sqlx::query("SELECT f.number,f.level,e.name FROM findings f JOIN elevations e ON e.id=f.elevation WHERE f.id=?").bind(id).fetch_optional(&self.pool).await? {return Ok(json!({"id":format!("F{:03}",r.get::<i64,_>("number")),"elevation":r.get::<String,_>("name"),"level":r.get::<Option<String>,_>("level")}));}
        }
        Ok(Value::Null)
    }
    async fn evidence_json(&self, id: i64) -> Result<Value> {
        if let Some(e) = self.evidence(id).await? {
            let name: Option<String> = if let Some(id) = e.elevation {
                sqlx::query_scalar("SELECT name FROM elevations WHERE id=?")
                    .bind(id)
                    .fetch_optional(&self.pool)
                    .await?
            } else {
                None
            };
            return Ok(
                json!({"id":format!("E{:04}",e.id),"project":e.project,"label":e.label,"finding":self.finding_json(e.finding).await?,"elevation":name,"has_photo":e.path.is_some(),"description":e.description}),
            );
        }
        Ok(Value::Null)
    }
    pub async fn save_analysis(
        &self,
        job: &Job,
        analysis: &crate::deepseek::Analysis,
    ) -> Result<()> {
        sqlx::query("UPDATE messages SET analysis=?,model=?,usage=?,error=NULL WHERE id=?")
            .bind(serde_json::to_string(&analysis.proposal)?)
            .bind(&analysis.model)
            .bind(analysis.usage.to_string())
            .bind(job.id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn retry(&self, job: &Job, error: &str) -> Result<()> {
        let attempts = job.attempts + 1;
        let status = if attempts >= 5 { "failed" } else { "pending" };
        sqlx::query("UPDATE messages SET attempts=?,next_attempt=?,status=?,error=? WHERE id=?")
            .bind(attempts)
            .bind(now() + 30 * 2i64.pow(attempts.min(6) as u32))
            .bind(status)
            .bind(error)
            .bind(job.id)
            .execute(&self.pool)
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
        sqlx::query(
            "INSERT INTO outbox(chat,topic,reply_to,kind,text,path,evidence) VALUES(?,?,?,?,?,?,?)",
        )
        .bind(job.chat)
        .bind(job.topic)
        .bind(job.telegram_id)
        .bind(kind)
        .bind(text)
        .bind(path)
        .bind(evidence)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
    pub async fn apply(&self, job: &Job, p: &Proposal, context: &Value) -> Result<()> {
        if p.action == Action::Ignore && self.current_evidence(job).await?.is_none() {
            sqlx::query("UPDATE messages SET status='done' WHERE id=?")
                .bind(job.id)
                .execute(&self.pool)
                .await?;
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        let mut project = context["active_project"].as_str().map(str::to_owned);
        let mut receipt = String::new();
        let mut question = None;
        let mut changed = false;
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
            target=sqlx::query_as::<_,Evidence>("SELECT e.id,e.message,e.project,e.elevation,e.finding,e.label,e.description,e.path,e.preview_path,e.yolo_attempts FROM evidence e JOIN messages m ON m.id=e.message WHERE e.id=? AND m.chat=? AND m.topic=?")
       .bind(id).bind(job.chat).bind(job.topic).fetch_optional(&mut *tx).await?;
            ensure!(target.is_some(), "evidence is outside this chat/topic");
        }
        match p.action {
            Action::SelectProject => {
                if let Some(location) = p.location.as_deref().filter(|v| !normalize(v).is_empty()) {
                    let key = normalize(location);
                    ensure!(key.len() <= 200, "location too long");
                    let existing: Option<String> = sqlx::query_scalar(
                        "SELECT project FROM aliases WHERE chat=? AND location_key=?",
                    )
                    .bind(job.chat)
                    .bind(&key)
                    .fetch_optional(&mut *tx)
                    .await?;
                    if let Some(id) = existing {
                        project = Some(id);
                    } else {
                        let sites=sqlx::query("SELECT id,location,location_key FROM projects WHERE owner_chat=? AND archived=0").bind(job.chat).fetch_all(&mut *tx).await?;
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
                            let id = uuid::Uuid::new_v4().to_string();
                            sqlx::query("INSERT INTO projects(id,location,location_key,owner_chat,created_at) VALUES(?,?,?,?,?)").bind(&id).bind(location.trim()).bind(&key).bind(job.chat).bind(now()).execute(&mut *tx).await?;
                            sqlx::query(
                                "INSERT INTO aliases(chat,location_key,project) VALUES(?,?,?)",
                            )
                            .bind(job.chat)
                            .bind(&key)
                            .bind(&id)
                            .execute(&mut *tx)
                            .await?;
                            project = Some(id);
                            receipt.push_str("Created project. ");
                        }
                    }
                    if question.is_none() {
                        let id = project.as_ref().unwrap();
                        let archived: i64 =
                            sqlx::query_scalar("SELECT archived FROM projects WHERE id=?")
                                .bind(id)
                                .fetch_one(&mut *tx)
                                .await?;
                        if archived == 1 {
                            question = Some(
                                "This project is archived. Ask me to restore it first.".into(),
                            );
                            project = context["active_project"].as_str().map(str::to_owned);
                        } else {
                            sqlx::query("INSERT INTO chats(chat,topic,project) VALUES(?,?,?) ON CONFLICT(chat,topic) DO UPDATE SET project=excluded.project")
               .bind(job.chat).bind(job.topic).bind(id).execute(&mut *tx).await?;
                            receipt.push_str(&format!("Selected {location}."));
                            changed = true;
                            if context["active_project"].as_str() != Some(id.as_str()) {
                                sqlx::query("DELETE FROM contexts WHERE chat=? AND topic=?")
                                    .bind(job.chat)
                                    .bind(job.topic)
                                    .execute(&mut *tx)
                                    .await?;
                            }
                        }
                    }
                } else {
                    question = Some("What is the project location or building name?".into());
                }
            }
            Action::ListProjects => {
                let sites = sqlx::query(
                    "SELECT location,archived FROM projects WHERE owner_chat=? ORDER BY location",
                )
                .bind(job.chat)
                .fetch_all(&mut *tx)
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
                                if r.get::<i64, _>("archived") == 1 {
                                    " (archived)"
                                } else {
                                    ""
                                }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                };
            }
            Action::RenameProject | Action::ArchiveProject | Action::RestoreProject => {
                let id = if let Some(loc) = &p.location {
                    sqlx::query_scalar::<_, String>(
                        "SELECT project FROM aliases WHERE chat=? AND location_key=?",
                    )
                    .bind(job.chat)
                    .bind(normalize(loc))
                    .fetch_optional(&mut *tx)
                    .await?
                } else {
                    project.clone()
                };
                if let Some(id) = id {
                    if p.action == Action::RenameProject {
                        if let Some(loc) = p
                            .new_location
                            .as_deref()
                            .filter(|v| !normalize(v).is_empty())
                        {
                            let key = normalize(loc);
                            ensure!(key.len() <= 200, "location too long");
                            let conflict: Option<String> = sqlx::query_scalar(
                                "SELECT project FROM aliases WHERE chat=? AND location_key=?",
                            )
                            .bind(job.chat)
                            .bind(&key)
                            .fetch_optional(&mut *tx)
                            .await?;
                            if conflict.as_deref().is_some_and(|v| v != id) {
                                question=Some("That location already belongs to another project. Please choose a different name.".into());
                            } else {
                                sqlx::query(
                                    "UPDATE projects SET location=?,location_key=? WHERE id=?",
                                )
                                .bind(loc.trim())
                                .bind(&key)
                                .bind(&id)
                                .execute(&mut *tx)
                                .await?;
                                sqlx::query("INSERT OR IGNORE INTO aliases(chat,location_key,project) VALUES(?,?,?)").bind(job.chat).bind(key).bind(&id).execute(&mut *tx).await?;
                                receipt = format!(
                                    "Renamed project to {loc}; previous names remain aliases."
                                );
                                changed = true;
                            }
                        } else {
                            question = Some("What should the new location name be?".into());
                        }
                    } else {
                        let archived = i64::from(p.action == Action::ArchiveProject);
                        sqlx::query("UPDATE projects SET archived=? WHERE id=?")
                            .bind(archived)
                            .bind(&id)
                            .execute(&mut *tx)
                            .await?;
                        if archived == 1 {
                            sqlx::query("UPDATE chats SET project=NULL WHERE project=?")
                                .bind(&id)
                                .execute(&mut *tx)
                                .await?;
                            sqlx::query("DELETE FROM contexts WHERE project=?")
                                .bind(&id)
                                .execute(&mut *tx)
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
                question = Some(p.question.clone().unwrap_or_else(|| {
                    "Please specify the project, elevation or photo label.".into()
                }));
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
            || p.action == Action::ShowChecklist;
        if needs_project && project.is_none() && question.is_none() {
            question = Some(
                "What is the building name or location? Say ‘new project, Star Vista’.".into(),
            );
        }
        if let Some(ref project) = project {
            if question.is_none() {
                let archived: i64 =
                    sqlx::query_scalar("SELECT archived FROM projects WHERE id=? AND owner_chat=?")
                        .bind(project)
                        .bind(job.chat)
                        .fetch_one(&mut *tx)
                        .await?;
                if archived != 0 {
                    question = Some("This project is archived. Restore it to add evidence.".into());
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
                        receipt = "Elevations added.".into();
                    }
                    if needs_assignment {
                        if let Some(ref e) = target {
                            if e.project.as_deref().is_some_and(|v| v != project) {
                                question=Some("That evidence belongs to a different project. Select its project before correcting it.".into());
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
                            let row = sqlx::query(
                                "SELECT id,elevation FROM findings WHERE project=? AND number=?",
                            )
                            .bind(project)
                            .bind(number)
                            .fetch_optional(&mut *tx)
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
                                    finding = sqlx::query_scalar(
                                        "SELECT id FROM findings WHERE project=? AND number=?",
                                    )
                                    .bind(project)
                                    .bind(number)
                                    .fetch_optional(&mut *tx)
                                    .await?;
                                }
                            }
                        }
                        if let Some(fid) = finding {
                            let fe: i64 =
                                sqlx::query_scalar("SELECT elevation FROM findings WHERE id=?")
                                    .bind(fid)
                                    .fetch_one(&mut *tx)
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
                                let number:i64=sqlx::query_scalar("SELECT COALESCE(MAX(number),0)+1 FROM findings WHERE project=?").bind(project).fetch_one(&mut *tx).await?;
                                finding=Some(sqlx::query("INSERT INTO findings(project,number,elevation,level,created_at) VALUES(?,?,?,?,?)").bind(project).bind(number).bind(elevation_id).bind(&p.level).bind(now()).execute(&mut *tx).await?.last_insert_rowid());
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
                                    sqlx::query("UPDATE evidence SET project=?,elevation=?,finding=?,label=?,assignment_source=?,status=? WHERE id=?")
                  .bind(project).bind(if is_elevation{elevation_id}else{None}).bind(if is_finding{finding}else{None}).bind(label.key()).bind(source).bind(if label==Label::Unrelated{"unrelated"}else{"assigned"}).bind(e.id).execute(&mut *tx).await?;
                                    sqlx::query("UPDATE messages SET status='done' WHERE id=? AND status IN ('pending','failed')").bind(e.message).execute(&mut *tx).await?;
                                    receipt.push_str(&format!(
                                        "\nSaved E{:04} as {}.",
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
                                sqlx::query("INSERT OR IGNORE INTO evidence(message,project,elevation,finding,description,status,assignment_source,created_at) VALUES(?,?,?,?,?,'assigned','text',?)")
                                    .bind(job.id).bind(project).bind(elevation_id).bind(fid).bind(description).bind(now()).execute(&mut *tx).await?;
                                let current_id: i64 =
                                    sqlx::query_scalar("SELECT id FROM evidence WHERE message=?")
                                        .bind(job.id)
                                        .fetch_one(&mut *tx)
                                        .await?;
                                sqlx::query("UPDATE evidence SET description=? WHERE id=?")
                                    .bind(description)
                                    .bind(current_id)
                                    .execute(&mut *tx)
                                    .await?;
                                sqlx::query("INSERT INTO descriptions(message,finding,evidence,text,created_at) VALUES(?,?,?,?,?) ON CONFLICT(message,finding) DO UPDATE SET text=excluded.text")
                                    .bind(job.id).bind(fid).bind(current_id).bind(description).bind(now()).execute(&mut *tx).await?;
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
                    sqlx::query("UPDATE evidence SET project=? WHERE id=?")
                        .bind(&project)
                        .bind(e.id)
                        .execute(&mut *tx)
                        .await?;
                }
            }
        }
        let eid = target.as_ref().map(|e| e.id);
        if needs_assignment || question.is_some() || finding.is_some() {
            sqlx::query("INSERT INTO contexts(chat,topic,sender,project,finding,evidence,prompt) VALUES(?,?,?,?,?,?,?) ON CONFLICT(chat,topic,sender) DO UPDATE SET project=excluded.project,finding=COALESCE(excluded.finding,contexts.finding),evidence=excluded.evidence,prompt=excluded.prompt")
       .bind(job.chat).bind(job.topic).bind(&job.sender).bind(&project).bind(finding).bind(if question.is_some(){eid}else{None}).bind(&question).execute(&mut *tx).await?;
        }
        if changed {
            sqlx::query("INSERT INTO audit(message,evidence,action,before_json,after_json,created_at) VALUES(?,?,?,?,?,?)").bind(job.id).bind(eid).bind(format!("{:?}",p.action)).bind(context.to_string()).bind(serde_json::to_string(p)?).bind(now()).execute(&mut *tx).await?;
        }
        if let Some(q) = question {
            if !receipt.trim().is_empty() {
                receipt.push('\n');
            }
            receipt.push_str(&q);
        }
        if changed && receipt.trim().is_empty() {
            receipt = "Checklist updated.".into();
        }
        if !receipt.trim().is_empty() {
            queue(&mut tx, job, "reply", receipt.trim(), eid).await?;
        }
        if changed || p.action == Action::ShowChecklist {
            queue(&mut tx, job, "checklist", "", None).await?;
        }
        sqlx::query("UPDATE messages SET status='done',error=NULL WHERE id=?")
            .bind(job.id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        if let Some(fid) = finding {
            if self.prompt_finding(job, fid).await.is_err() {
                tracing::warn!("Could not queue follow-up prompt");
            }
        }
        Ok(())
    }
    async fn prompt_finding(&self, job: &Job, fid: i64) -> Result<()> {
        let r=sqlx::query("SELECT number, EXISTS(SELECT 1 FROM evidence WHERE finding=f.id AND label='context_photo' AND path IS NOT NULL) AS context_done, EXISTS(SELECT 1 FROM evidence WHERE finding=f.id AND label='close_up' AND path IS NOT NULL) AS close_done, EXISTS(SELECT 1 FROM descriptions WHERE finding=f.id AND text<>'') AS description_done FROM findings f WHERE id=?").bind(fid).fetch_one(&self.pool).await?;
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
            let previous: Option<i64> = sqlx::query_scalar::<_, Option<i64>>(
                "SELECT prompted_finding FROM contexts WHERE chat=? AND topic=? AND sender=?",
            )
            .bind(job.chat)
            .bind(job.topic)
            .bind(&job.sender)
            .fetch_optional(&self.pool)
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
                sqlx::query(
                    "UPDATE contexts SET prompted_finding=? WHERE chat=? AND topic=? AND sender=?",
                )
                .bind(fid)
                .bind(job.chat)
                .bind(job.topic)
                .bind(&job.sender)
                .execute(&self.pool)
                .await?;
            }
        }
        Ok(())
    }
    pub async fn checklist(&self, project: Option<&str>) -> Result<String> {
        let Some(project) = project else {
            return Ok("No active project. Say ‘new project, Star Vista’.".into());
        };
        let location: String = sqlx::query_scalar("SELECT location FROM projects WHERE id=?")
            .bind(project)
            .fetch_one(&self.pool)
            .await?;
        let mut output = format!("DataDog · {location}\nEvidence checklist\n");
        for (key, title) in [
            ("building_overview", "Building overview"),
            ("plan", "Plan drawing"),
        ] {
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM evidence WHERE project=? AND label=? AND path IS NOT NULL",
            )
            .bind(project)
            .bind(key)
            .fetch_one(&self.pool)
            .await?;
            output.push_str(&format!("{} {title}\n", if count > 0 { "☑" } else { "☐" }));
        }
        let elevations = sqlx::query("SELECT id,name FROM elevations WHERE project=? ORDER BY id")
            .bind(project)
            .fetch_all(&self.pool)
            .await?;
        if elevations.is_empty() {
            output.push_str(
                "\nTell me which elevations to track, e.g. ‘East and West elevations’.\n",
            );
        }
        for e in elevations {
            let id: i64 = e.get("id");
            let count:i64=sqlx::query_scalar("SELECT count(*) FROM evidence WHERE project=? AND elevation=? AND label='elevation_overview' AND path IS NOT NULL").bind(project).bind(id).fetch_one(&self.pool).await?;
            output.push_str(&format!(
                "\n{}\n{} Elevation overview\n",
                e.get::<String, _>("name"),
                if count > 0 { "☑" } else { "☐" }
            ));
            let findings = sqlx::query(
                "SELECT id,number,level FROM findings WHERE elevation=? ORDER BY number",
            )
            .bind(id)
            .fetch_all(&self.pool)
            .await?;
            for f in findings {
                let fid: i64 = f.get("id");
                output.push_str(&format!(
                    "F{:03}{}\n",
                    f.get::<i64, _>("number"),
                    f.get::<Option<String>, _>("level")
                        .map(|v| format!(" — {v}"))
                        .unwrap_or_default()
                ));
                for (key, title) in [
                    ("context_photo", "Context photo"),
                    ("close_up", "Close-up photo"),
                    ("description", "Description"),
                ] {
                    let count: i64 = if key == "description" {
                        sqlx::query_scalar("SELECT count(*) FROM descriptions d JOIN findings f ON f.id=d.finding WHERE f.project=? AND d.finding=? AND d.text<>''").bind(project).bind(fid).fetch_one(&self.pool).await?
                    } else {
                        sqlx::query_scalar("SELECT count(*) FROM evidence WHERE project=? AND finding=? AND label=? AND path IS NOT NULL").bind(project).bind(fid).bind(key).fetch_one(&self.pool).await?
                    };
                    output.push_str(&format!(
                        "  {} {title}\n",
                        if count > 0 { "☑" } else { "☐" }
                    ));
                }
            }
        }
        let unassigned:i64=sqlx::query_scalar("SELECT count(*) FROM evidence WHERE project=? AND status='unassigned' AND path IS NOT NULL").bind(project).fetch_one(&self.pool).await?;
        if unassigned > 0 {
            output.push_str(&format!("\n{unassigned} saved photo(s) awaiting labels.\n"));
        }
        output.push_str("\nTicks indicate saved evidence.");
        Ok(output)
    }
    pub async fn next_outgoing(&self) -> Result<Option<Outgoing>> {
        Ok(sqlx::query_as("SELECT id,chat,topic,reply_to,kind,text,path,evidence,attempts FROM outbox WHERE delivered_at IS NULL AND next_attempt<=? ORDER BY id LIMIT 1").bind(now()).fetch_optional(&self.pool).await?)
    }
}
async fn elevation(tx: &mut Transaction<'_, Sqlite>, project: &str, name: &str) -> Result<i64> {
    let key = normalize(name);
    ensure!(!key.is_empty() && key.len() <= 100, "invalid elevation");
    sqlx::query("INSERT OR IGNORE INTO elevations(project,name,name_key) VALUES(?,?,?)")
        .bind(project)
        .bind(name.trim())
        .bind(key.clone())
        .execute(&mut **tx)
        .await?;
    Ok(
        sqlx::query_scalar("SELECT id FROM elevations WHERE project=? AND name_key=?")
            .bind(project)
            .bind(key)
            .fetch_one(&mut **tx)
            .await?,
    )
}
async fn queue(
    tx: &mut Transaction<'_, Sqlite>,
    job: &Job,
    kind: &str,
    text: &str,
    evidence: Option<i64>,
) -> Result<()> {
    sqlx::query("INSERT INTO outbox(chat,topic,reply_to,kind,text,evidence) VALUES(?,?,?,?,?,?)")
        .bind(job.chat)
        .bind(job.topic)
        .bind(job.telegram_id)
        .bind(kind)
        .bind(text)
        .bind(evidence)
        .execute(&mut **tx)
        .await?;
    if kind == "checklist" {
        sqlx::query("UPDATE outbox SET next_attempt=? WHERE id=last_insert_rowid()")
            .bind(now() + 2)
            .execute(&mut **tx)
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
