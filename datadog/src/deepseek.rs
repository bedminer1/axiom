use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Ignore,
    SelectProject,
    CreateInspection,
    SelectInspection,
    ListInspections,
    ShowChecklist,
    ListProjects,
    AddElevation,
    CreateFinding,
    UpdateEvidence,
    RenameProject,
    ArchiveProject,
    RestoreProject,
    AddTeamMember,
    RemoveTeamMember,
    ShowTeam,
    GenerateReport,
    Clarify,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Label {
    BuildingOverview,
    Plan,
    ElevationOverview,
    ContextPhoto,
    CloseUp,
    Unrelated,
    Uncertain,
}
impl Label {
    pub fn key(self) -> &'static str {
        match self {
            Self::BuildingOverview => "building_overview",
            Self::Plan => "plan",
            Self::ElevationOverview => "elevation_overview",
            Self::ContextPhoto => "context_photo",
            Self::CloseUp => "close_up",
            Self::Unrelated => "unrelated",
            Self::Uncertain => "uncertain",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::BuildingOverview => "building overview",
            Self::Plan => "plan drawing",
            Self::ElevationOverview => "elevation overview",
            Self::ContextPhoto => "context photo",
            Self::CloseUp => "close-up photo",
            Self::Unrelated => "unrelated photo",
            Self::Uncertain => "unassigned photo",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub action: Action,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub new_location: Option<String>,
    #[serde(default)]
    pub inspection_reference: Option<String>,
    #[serde(default)]
    pub inspection_title: Option<String>,
    #[serde(default)]
    pub elevations: Vec<String>,
    #[serde(default)]
    pub elevation: Option<String>,
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub finding: Option<String>,
    #[serde(default)]
    pub new_finding: bool,
    #[serde(default)]
    pub evidence: Option<String>,
    #[serde(default)]
    pub label: Option<Label>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub question: Option<String>,
    #[serde(default)]
    pub certain: bool,
    #[serde(default)]
    pub observations: Vec<VisualObservation>,
    #[serde(default)]
    pub field_note: Option<String>,
    #[serde(default)]
    pub zone: Option<String>,
    #[serde(default)]
    pub member: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualObservation {
    pub component: String,
    pub condition: String,
    pub note: String,
    pub confidence: f32,
    #[serde(default)]
    pub verification: Vec<String>,
}
#[derive(Clone)]
pub struct DeepSeek {
    client: reqwest::Client,
    key: Arc<String>,
    base: String,
    pub model: String,
}
pub struct Analysis {
    pub proposal: Proposal,
    pub model: String,
    pub usage: Value,
}
impl DeepSeek {
    pub fn new(key: String, base: String, model: String) -> Result<Self> {
        ensure!(!key.trim().is_empty(), "DEEPSEEK_API_KEY is missing");
        // A configured endpoint receives the key and evidence; require HTTPS.
        ensure!(
            base.starts_with("https://"),
            "DEEPSEEK_BASE_URL must use HTTPS"
        );
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(150))
                .connect_timeout(Duration::from_secs(15))
                .build()?,
            key: Arc::new(key),
            base: base.trim_end_matches('/').to_owned(),
            model,
        })
    }
    pub async fn analyze(
        &self,
        context: Value,
        text: &str,
        image: Option<&[u8]>,
    ) -> Result<Analysis> {
        let mut content = vec![
            json!({"type":"text", "text":json!({"context":context,"message":text}).to_string()}),
        ];
        if let Some(image) = image {
            content.push(json!({"type":"image_url", "image_url":{"url":format!("data:image/jpeg;base64,{}", STANDARD.encode(image)),"detail":"high"}}));
        }
        let has_image = image.is_some();
        let response = self.client.post(format!("{}/chat/completions", self.base))
            .bearer_auth(self.key.as_str()).json(&json!({
                "model":self.model,
                "thinking":{"type":"enabled"},
                "reasoning_effort":if has_image { "high" } else { "low" },
                "max_tokens":if has_image { 4096 } else { 1024 }, "response_format":{"type":"json_object"},
                "messages":[{"role":"system","content":include_str!("../prompts/collector.txt")},{"role":"user","content":content}]
            })).send().await.map_err(|_|anyhow::anyhow!("DeepSeek connection or timeout failure"))?;
        let status = response.status();
        if !status.is_success() {
            bail!("DeepSeek HTTP {}", status.as_u16());
        }
        let value: Value = response
            .json()
            .await
            .context("invalid DeepSeek response JSON")?;
        ensure!(
            value["choices"][0]["finish_reason"] == "stop",
            "DeepSeek response incomplete"
        );
        let content = value["choices"][0]["message"]["content"]
            .as_str()
            .context("DeepSeek returned no content")?;
        let proposal: Proposal =
            serde_json::from_str(content).context("DeepSeek returned an invalid action")?;
        if let Some(id) = &proposal.evidence {
            ensure!(
                crate::store::parse_id(id, 'E').is_some(),
                "invalid evidence ID"
            );
        }
        if let Some(id) = &proposal.finding {
            ensure!(
                crate::store::parse_id(id, 'F').is_some(),
                "invalid finding ID"
            );
        }
        ensure!(
            proposal.elevations.len() <= 32,
            "too many elevations in one action"
        );
        ensure!(
            proposal.observations.len() <= 30,
            "too many visual observations"
        );
        for observation in &proposal.observations {
            ensure!(
                (observation.confidence.is_finite()
                    && (0.0..=1.0).contains(&observation.confidence)),
                "invalid visual observation confidence"
            );
            ensure!(
                observation.note.chars().count() <= 1000 && !observation.note.contains('\0'),
                "invalid visual observation note"
            );
            ensure!(
                observation.verification.len() <= 8,
                "too many verification methods"
            );
        }
        for value in [
            &proposal.location,
            &proposal.new_location,
            &proposal.inspection_reference,
            &proposal.inspection_title,
            &proposal.elevation,
            &proposal.level,
            &proposal.finding,
            &proposal.evidence,
            &proposal.description,
            &proposal.question,
            &proposal.field_note,
            &proposal.zone,
            &proposal.member,
        ]
        .into_iter()
        .flatten()
        .chain(proposal.elevations.iter())
        {
            ensure!(
                value.chars().count() <= 2000 && !value.contains('\0'),
                "invalid action field"
            );
        }
        Ok(Analysis {
            proposal,
            model: value["model"].as_str().unwrap_or(&self.model).to_owned(),
            usage: value["usage"].clone(),
        })
    }
}
