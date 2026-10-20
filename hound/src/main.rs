mod filter;
mod llm;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use llm::Llm;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{path::PathBuf, process::Stdio, sync::Arc};
use tokio::{net::TcpListener, process::Command};

#[derive(Clone)]
struct AppState {
    model: Llm,
}

struct ApiError(anyhow::Error);

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = if self.0.to_string().contains("SoCLaas") {
            StatusCode::BAD_GATEWAY
        } else {
            StatusCode::BAD_REQUEST
        };
        (status, Json(json!({ "error": self.0.to_string() }))).into_response()
    }
}

#[derive(Deserialize)]
struct PhotoRequest {
    slot: String,
    image: String,
    filename: Option<String>,
    project: Value,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let state = Arc::new(AppState {
        model: Llm::from_environment()?,
    });
    let app = Router::new()
        .route(
            "/api/health",
            get(|| async { Json(json!({"status": "ok"})) }),
        )
        .route("/api/photo/analyze", post(analyze_photo))
        .layer(DefaultBodyLimit::max(25_000_000))
        .with_state(state);
    let address = std::env::var("HOUND_API_ADDRESS").unwrap_or_else(|_| "127.0.0.1:5050".into());
    let listener = TcpListener::bind(&address).await?;
    if std::env::var("HOUND_START_WEBSITE").as_deref() == Ok("0") {
        println!("Hound API listening on {address}");
        tokio::select! {
            result = axum::serve(listener, app) => result?,
            _ = tokio::signal::ctrl_c() => {}
        }
        return Ok(());
    }
    let site = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("website");
    let vite = site.join("node_modules/vite/bin/vite.js");
    anyhow::ensure!(
        vite.exists(),
        "website dependencies missing; run npm install in website/"
    );
    let mut website = Command::new("node")
        .arg(vite)
        .arg("--host")
        .arg("127.0.0.1")
        .current_dir(&site)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    println!("Hound API listening on {address}; starting website…");
    tokio::select! {
        result = axum::serve(listener, app) => result?,
        result = website.wait() => anyhow::bail!("website stopped: {}", result?),
        _ = tokio::signal::ctrl_c() => {}
    }
    Ok(())
}

async fn analyze_photo(
    State(state): State<Arc<AppState>>,
    Json(request): Json<PhotoRequest>,
) -> Result<Json<Value>, ApiError> {
    if request.slot.len() > 120 {
        return Err(ApiError(anyhow::anyhow!("photo slot is too long")));
    }
    if request.image.len() > 24_000_000 {
        return Err(ApiError(anyhow::anyhow!("photo is too large")));
    }
    if ![
        "data:image/jpeg;base64,",
        "data:image/png;base64,",
        "data:image/webp;base64,",
        "data:image/gif;base64,",
    ]
    .iter()
    .any(|prefix| request.image.starts_with(prefix))
    {
        return Err(ApiError(anyhow::anyhow!("unsupported photo format")));
    }
    let (section, elevation) = request
        .slot
        .split_once(':')
        .ok_or_else(|| ApiError(anyhow::anyhow!("invalid photo slot")))?;
    if !["visual", "close"].contains(&section)
        || (section == "visual"
            && !request.project["elevations"]
                .as_array()
                .is_some_and(|items| items.iter().any(|value| value == elevation)))
    {
        return Err(ApiError(anyhow::anyhow!(
            "photo slot is not in this project"
        )));
    }
    let (_, encoded) = request.image.split_once(',').expect("validated data URL");
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| ApiError(anyhow::anyhow!("invalid photo data")))?;
    let filtered = filter::analyze(
        &request.project,
        &request.slot,
        request.filename.as_deref(),
        &bytes,
        &state.model,
    )
    .await;
    let mut result = match filtered {
        Ok(value) => value,
        Err(error) => {
            eprintln!("pfi-filter unavailable: {error:#}");
            json!({"analysis": "", "source": "llm-fallback"})
        }
    };
    if result["analysis"].as_str().unwrap_or("").is_empty() {
        result["analysis"] = json!(
            state
                .model
                .analyze_photo(&request.slot, &request.image)
                .await?
        );
        result["source"] = json!("llm-fallback");
    }
    let observation = result["analysis"].as_str().unwrap_or("");
    let defects = &result["filter"]["defects"];
    let lower = observation.to_lowercase();
    let likely_defect = defects.as_array().is_some_and(|items| !items.is_empty())
        || (!lower.contains("no visible defect")
            && !lower.contains("no crack")
            && ["crack", "spall", "loose", "corros", "detach", "debris"]
                .iter()
                .any(|term| lower.contains(term)));
    if likely_defect {
        match state.model.suggest_repair(observation, defects).await {
            Ok(suggestion) => result["repair_suggestion"] = suggestion,
            Err(error) => eprintln!("repair suggestion unavailable: {error:#}"),
        }
    }
    Ok(Json(result))
}
