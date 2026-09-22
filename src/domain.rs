//! Shared API domain types.
//!
//! Defines model selection, per-photo analysis, geometry metrics, and the
//! explicitly simulated assessment values used by the demo UI.

use serde::{Deserialize, Serialize};

/// Identifies the Python segmentation backend selected for one request.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Model {
    #[default]
    Unet,
    Sam3,
}

/// Carries the runtime model choice from the browser to the analysis route.
#[derive(Debug, Default, Deserialize)]
pub struct AnalyzeRequest {
    #[serde(default)]
    pub model: Model,
}

/// Reports server readiness and the runtime-selectable model set.
#[derive(Serialize)]
pub struct Health {
    pub status: &'static str,
    pub dataset_ready: bool,
    pub models: [Model; 2],
}

/// Describes the JSON contract emitted by either Python worker.
#[derive(Deserialize)]
pub struct WorkerOutput {
    pub mask_path: String,
    pub score: f32,
    pub instances: usize,
    pub model: String,
}

/// Represents a complete single-photo response consumed by the Svelte UI.
#[derive(Serialize)]
pub struct Analysis {
    pub image_url: String,
    pub mask_url: String,
    pub source_name: String,
    pub model: String,
    pub confidence: f32,
    pub instances: usize,
    pub metrics: Metrics,
    pub grade: Grade,
}

/// Collects measured pixel geometry and demo-only physical estimates.
#[derive(Serialize)]
pub struct Metrics {
    pub coverage_percent: f64,
    pub estimated_length_px: f64,
    pub demo_estimated_length_mm: f64,
    pub length_percent_diagonal: f64,
    pub mean_width_px: f64,
    pub demo_mean_width_mm: f64,
    pub mean_width_percent_short_side: f64,
    pub dominant_orientation_deg: f64,
    pub orientation_from_horizontal_deg: f64,
    pub orientation_label: &'static str,
    pub foreground_pixels: u64,
    pub demo_scale_mm_per_px: f64,
}

/// Labels a simulated demo severity without claiming engineering calibration.
#[derive(Serialize)]
pub struct Grade {
    pub label: &'static str,
    pub rationale: String,
    pub calibrated: bool,
    pub simulated: bool,
}
