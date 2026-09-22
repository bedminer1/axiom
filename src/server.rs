//! Axum server, routes, and static-file delivery.
//!
//! Owns application state, selects random dataset photos, delegates mask
//! generation to the ML layer, and returns browser-ready analysis JSON.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use rand::prelude::IndexedRandom;
use tower_http::{cors::CorsLayer, services::ServeDir, trace::TraceLayer};
use walkdir::WalkDir;

use crate::{
    domain::{Analysis, AnalyzeRequest, Health, Model},
    geometry::{demo_grade, measure_mask},
    ml::run_worker,
};

/// Stores immutable repository paths shared by request handlers.
#[derive(Clone)]
struct AppState {
    root: PathBuf,
}

/// Builds and serves the HTTP application on the local demo address.
pub async fn serve(root: PathBuf) -> Result<()> {
    tokio::fs::create_dir_all(root.join("data/artifacts")).await?;
    let state = AppState { root: root.clone() };
    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/analyze/random", post(analyze_random))
        .nest_service("/dataset", ServeDir::new(root.join("data/dataset")))
        .nest_service("/artifacts", ServeDir::new(root.join("data/artifacts")))
        .fallback_service(
            ServeDir::new(root.join("web/build")).append_index_html_on_directories(true),
        )
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);
    let address = "127.0.0.1:3000";
    tracing::info!("Axiom listening at http://{address}");
    axum::serve(tokio::net::TcpListener::bind(address).await?, app).await?;
    Ok(())
}

/// Returns dataset readiness and models selectable after launch.
async fn health(State(state): State<AppState>) -> Json<Health> {
    let ready = image_files(&state.root.join("data/dataset"))
        .map(|images| !images.is_empty())
        .unwrap_or(false);
    Json(Health {
        status: "ok",
        dataset_ready: ready,
        models: [Model::Unet, Model::Sam3],
    })
}

/// Handles one random photo using the model supplied in the request.
async fn analyze_random(
    State(state): State<AppState>,
    Json(request): Json<AnalyzeRequest>,
) -> impl IntoResponse {
    match analyze(&state, request.model).await {
        Ok(value) => (StatusCode::OK, Json(serde_json::json!(value))).into_response(),
        Err(error) => {
            tracing::error!(?error, "analysis failed");
            (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({
                "error": error.to_string(),
                "hint": "Run ./scripts/setup-dataset.sh and check the selected model setup in README.md."
            }))).into_response()
        }
    }
}

/// Selects an image, runs segmentation, and assembles its response.
async fn analyze(state: &AppState, model: Model) -> Result<Analysis> {
    let images = image_files(&state.root.join("data/dataset"))?;
    let image_path = images
        .choose(&mut rand::rng())
        .context("dataset is empty; run ./scripts/setup-dataset.sh")?
        .clone();
    let id = format!(
        "{}-{}",
        image_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("sample"),
        rand::random::<u32>()
    );
    let mask_path = state
        .root
        .join("data/artifacts")
        .join(format!("{id}-mask.png"));
    let worker = run_worker(&state.root, model, &image_path, &mask_path).await?;
    let resolved_mask = PathBuf::from(&worker.mask_path);
    let metrics = measure_mask(&resolved_mask)?;
    let grade = demo_grade(&metrics);
    let relative = image_path
        .strip_prefix(state.root.join("data/dataset"))?
        .to_string_lossy()
        .replace('\\', "/");
    Ok(Analysis {
        image_url: format!("/dataset/{relative}"),
        mask_url: format!(
            "/artifacts/{}",
            resolved_mask.file_name().unwrap().to_string_lossy()
        ),
        source_name: image_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        model: worker.model,
        confidence: worker.score,
        instances: worker.instances,
        metrics,
        grade,
    })
}

/// Recursively discovers supported image files in the dataset directory.
fn image_files(root: &Path) -> Result<Vec<PathBuf>> {
    if !root.exists() {
        return Ok(vec![]);
    }
    Ok(WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| {
            matches!(
                path.extension()
                    .and_then(|value| value.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some("jpg" | "jpeg" | "png" | "webp")
            )
        })
        .collect())
}
