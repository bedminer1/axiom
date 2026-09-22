use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
};

use anyhow::{Context, Result, bail};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use image::GrayImage;
use rand::prelude::IndexedRandom;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tower_http::{cors::CorsLayer, services::ServeDir, trace::TraceLayer};
use walkdir::WalkDir;

#[derive(Clone)]
struct AppState {
    root: PathBuf,
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    dataset_ready: bool,
    sam_command: String,
}

#[derive(Deserialize)]
struct WorkerOutput {
    mask_path: String,
    score: f32,
    instances: usize,
    model: String,
}

#[derive(Serialize)]
struct Analysis {
    image_url: String,
    mask_url: String,
    source_name: String,
    model: String,
    confidence: f32,
    instances: usize,
    metrics: Metrics,
    grade: Grade,
}

#[derive(Serialize)]
struct Metrics {
    coverage_percent: f64,
    estimated_length_px: f64,
    mean_width_px: f64,
    dominant_orientation_deg: f64,
    foreground_pixels: u64,
}

#[derive(Serialize)]
struct Grade {
    label: &'static str,
    rationale: String,
    calibrated: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "axiom=info,tower_http=info".into()),
        )
        .init();
    let root = env::current_dir()?.canonicalize()?;
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

async fn health(State(state): State<AppState>) -> Json<Health> {
    let ready = image_files(&state.root.join("data/dataset"))
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    Json(Health {
        status: "ok",
        dataset_ready: ready,
        sam_command: sam_command(&state.root),
    })
}

async fn analyze_random(State(state): State<AppState>) -> impl IntoResponse {
    match analyze(&state).await {
        Ok(value) => (StatusCode::OK, Json(serde_json::json!(value))).into_response(),
        Err(error) => {
            tracing::error!(?error, "analysis failed");
            (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({
                "error": error.to_string(),
                "hint": "Run ./scripts/setup-dataset.sh, then configure a CUDA SAM 3 environment as described in README.md."
            }))).into_response()
        }
    }
}

async fn analyze(state: &AppState) -> Result<Analysis> {
    let images = image_files(&state.root.join("data/dataset"))?;
    let image_path = images
        .choose(&mut rand::rng())
        .context("dataset is empty; run ./scripts/setup-dataset.sh")?
        .clone();
    let id = format!(
        "{}-{}",
        image_path
            .file_stem()
            .and_then(|v| v.to_str())
            .unwrap_or("sample"),
        rand::random::<u32>()
    );
    let mask_path = state
        .root
        .join("data/artifacts")
        .join(format!("{id}-mask.png"));
    let command = sam_command(&state.root);
    let mut parts = command.split_whitespace();
    let program = parts.next().context("AXIOM_SAM_COMMAND is empty")?;
    let output = Command::new(program)
        .args(parts)
        .arg("--image")
        .arg(&image_path)
        .arg("--output")
        .arg(&mask_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .context("failed to start SAM 3 worker")?;
    if !output.status.success() {
        bail!(
            "SAM 3 worker failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let worker: WorkerOutput =
        serde_json::from_slice(&output.stdout).context("invalid SAM worker response")?;
    let resolved_mask = PathBuf::from(&worker.mask_path);
    let metrics = geometry(&resolved_mask)?;
    let grade = provisional_grade(&metrics);
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

fn sam_command(root: &Path) -> String {
    env::var("AXIOM_SAM_COMMAND").unwrap_or_else(|_| {
        format!(
            "uv run --project {} python -m axiom_ml.segment",
            root.join("ml").display()
        )
    })
}

fn image_files(root: &Path) -> Result<Vec<PathBuf>> {
    if !root.exists() {
        return Ok(vec![]);
    }
    Ok(WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| {
            matches!(
                p.extension()
                    .and_then(|v| v.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some("jpg" | "jpeg" | "png" | "webp")
            )
        })
        .collect())
}

fn geometry(path: &Path) -> Result<Metrics> {
    let mask = image::open(path)
        .with_context(|| format!("cannot read mask {}", path.display()))?
        .to_luma8();
    let (w, h) = mask.dimensions();
    let points: Vec<(f64, f64)> = mask
        .enumerate_pixels()
        .filter(|(_, _, p)| p[0] > 127)
        .map(|(x, y, _)| (x as f64, y as f64))
        .collect();
    if points.is_empty() {
        bail!("SAM 3 returned an empty crack mask");
    }
    let area = points.len() as f64;
    let (cx, cy) = points
        .iter()
        .fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    let (cx, cy) = (cx / area, cy / area);
    let (xx, yy, xy) = points.iter().fold((0.0, 0.0, 0.0), |a, p| {
        (
            a.0 + (p.0 - cx).powi(2),
            a.1 + (p.1 - cy).powi(2),
            a.2 + (p.0 - cx) * (p.1 - cy),
        )
    });
    let orientation = 0.5 * (2.0 * xy).atan2(xx - yy).to_degrees();
    let length = skeleton_length(&mask).max(1.0);
    Ok(Metrics {
        coverage_percent: area / (w as f64 * h as f64) * 100.0,
        estimated_length_px: length,
        mean_width_px: area / length,
        dominant_orientation_deg: if orientation < 0.0 {
            orientation + 180.0
        } else {
            orientation
        },
        foreground_pixels: points.len() as u64,
    })
}

fn skeleton_length(mask: &GrayImage) -> f64 {
    // A stable mask-centerline proxy: count foreground runs in both axes and combine them.
    let (w, h) = mask.dimensions();
    let mut horizontal = 0u64;
    let mut vertical = 0u64;
    for y in 0..h {
        for x in 0..w {
            if mask.get_pixel(x, y)[0] <= 127 {
                continue;
            }
            if x == 0 || mask.get_pixel(x - 1, y)[0] <= 127 {
                horizontal += 1;
            }
            if y == 0 || mask.get_pixel(x, y - 1)[0] <= 127 {
                vertical += 1;
            }
        }
    }
    ((horizontal.pow(2) + vertical.pow(2)) as f64).sqrt()
}

fn provisional_grade(m: &Metrics) -> Grade {
    let label = if m.coverage_percent < 0.5 {
        "minor"
    } else if m.coverage_percent < 2.0 {
        "moderate"
    } else {
        "severe"
    };
    Grade {
        label,
        calibrated: false,
        rationale: format!(
            "Screening grade from {:.2}% visible crack coverage. No physical scale or structural context; not an engineering assessment.",
            m.coverage_percent
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grade_boundaries() {
        let metric = |coverage| Metrics {
            coverage_percent: coverage,
            estimated_length_px: 1.0,
            mean_width_px: 1.0,
            dominant_orientation_deg: 0.0,
            foreground_pixels: 1,
        };
        assert_eq!(provisional_grade(&metric(0.49)).label, "minor");
        assert_eq!(provisional_grade(&metric(0.5)).label, "moderate");
        assert_eq!(provisional_grade(&metric(2.0)).label, "severe");
    }
}
