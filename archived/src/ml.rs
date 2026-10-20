//! Python machine-learning worker integration.
//!
//! Maps runtime model choices to Python commands, executes workers, records
//! latency, and validates their shared JSON response contract.

use std::{env, path::Path, process::Stdio, time::Instant};

use anyhow::{Context, Result, bail};
use tokio::process::Command;

use crate::domain::{Model, WorkerOutput};

/// Executes the selected segmentation worker for one source image.
pub async fn run_worker(
    root: &Path,
    model: Model,
    image_path: &Path,
    mask_path: &Path,
) -> Result<WorkerOutput> {
    let command = worker_command(root, model);
    let mut parts = command.split_whitespace();
    let program = parts.next().context("analysis worker command is empty")?;
    tracing::info!(?model, image = %image_path.display(), "starting analysis worker");
    let started = Instant::now();
    let output = Command::new(program)
        .args(parts)
        .arg("--image")
        .arg(image_path)
        .arg("--output")
        .arg(mask_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .context("failed to start analysis worker")?;
    tracing::info!(
        ?model,
        elapsed_ms = started.elapsed().as_millis(),
        success = output.status.success(),
        "analysis worker finished"
    );
    if !output.status.success() {
        bail!(
            "analysis worker failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    serde_json::from_slice(&output.stdout).context("invalid analysis worker response")
}

/// Resolves an environment override or the repository-local default command.
fn worker_command(root: &Path, model: Model) -> String {
    let (environment, module) = match model {
        Model::Unet => ("AXIOM_UNET_COMMAND", "axiom_ml.crack_segment"),
        Model::Sam3 => ("AXIOM_SAM_COMMAND", "axiom_ml.segment"),
    };
    env::var(environment).unwrap_or_else(|_| {
        format!(
            "uv run --project {} python -m {module}",
            root.join("ml").display()
        )
    })
}
