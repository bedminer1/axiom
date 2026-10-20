//! Binary-mask geometry and demo assessment calculations.
//!
//! Derives image-relative measurements from worker masks and adds clearly
//! marked simulated millimetre values for the product demonstration.

use anyhow::{Context, Result, bail};
use image::GrayImage;
use std::path::Path;

use crate::domain::{Grade, Metrics};

/// Demo-only conversion used when capture calibration is unavailable.
const DEMO_MM_PER_PIXEL: f64 = 0.12;

/// Loads a binary mask and derives aggregate geometry measurements.
pub fn measure_mask(path: &Path) -> Result<Metrics> {
    let mask = image::open(path)
        .with_context(|| format!("cannot read mask {}", path.display()))?
        .to_luma8();
    let (width, height) = mask.dimensions();
    let points: Vec<(f64, f64)> = mask
        .enumerate_pixels()
        .filter(|(_, _, pixel)| pixel[0] > 127)
        .map(|(x, y, _)| (x as f64, y as f64))
        .collect();
    if points.is_empty() {
        bail!("analysis worker returned an empty crack mask");
    }

    let area = points.len() as f64;
    let (center_x, center_y) = points
        .iter()
        .fold((0.0, 0.0), |sum, point| (sum.0 + point.0, sum.1 + point.1));
    let (center_x, center_y) = (center_x / area, center_y / area);
    let (xx, yy, xy) = points.iter().fold((0.0, 0.0, 0.0), |sum, point| {
        (
            sum.0 + (point.0 - center_x).powi(2),
            sum.1 + (point.1 - center_y).powi(2),
            sum.2 + (point.0 - center_x) * (point.1 - center_y),
        )
    });
    let raw_orientation = 0.5 * (2.0 * xy).atan2(xx - yy).to_degrees();
    let orientation = if raw_orientation < 0.0 {
        raw_orientation + 180.0
    } else {
        raw_orientation
    };
    let length = centerline_proxy(&mask).max(1.0);
    let mean_width = area / length;

    Ok(Metrics {
        coverage_percent: area / (width as f64 * height as f64) * 100.0,
        estimated_length_px: length,
        demo_estimated_length_mm: length * DEMO_MM_PER_PIXEL,
        length_percent_diagonal: length / (width as f64).hypot(height as f64) * 100.0,
        mean_width_px: mean_width,
        demo_mean_width_mm: mean_width * DEMO_MM_PER_PIXEL,
        mean_width_percent_short_side: mean_width / width.min(height) as f64 * 100.0,
        dominant_orientation_deg: orientation,
        orientation_from_horizontal_deg: orientation.min(180.0 - orientation),
        orientation_label: orientation_label(orientation),
        foreground_pixels: points.len() as u64,
        demo_scale_mm_per_px: DEMO_MM_PER_PIXEL,
    })
}

/// Converts a measured mask into an explicitly simulated demo severity class.
pub fn demo_grade(metrics: &Metrics) -> Grade {
    let label = match metrics.demo_mean_width_mm {
        width if width < 0.3 => "hairline",
        width if width < 1.0 => "moderate",
        _ => "wide",
    };
    Grade {
        label,
        calibrated: false,
        simulated: true,
        rationale: format!(
            "Demo classification from an assumed {:.2} mm/px scale and {:.2} mm estimated mean width. Not an engineering assessment.",
            metrics.demo_scale_mm_per_px, metrics.demo_mean_width_mm
        ),
    }
}

/// Maps an undirected principal-axis angle to a readable image direction.
fn orientation_label(angle: f64) -> &'static str {
    match angle {
        value if !(22.5..157.5).contains(&value) => "mostly horizontal",
        value if value < 67.5 => "diagonal, falling right",
        value if value < 112.5 => "mostly vertical",
        _ => "diagonal, rising right",
    }
}

/// Estimates total centerline length from foreground run starts in both axes.
fn centerline_proxy(mask: &GrayImage) -> f64 {
    let (width, height) = mask.dimensions();
    let mut horizontal = 0u64;
    let mut vertical = 0u64;
    for y in 0..height {
        for x in 0..width {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds representative metrics for demo-grade boundary tests.
    fn metrics_with_width(width_mm: f64) -> Metrics {
        Metrics {
            coverage_percent: 1.0,
            estimated_length_px: 10.0,
            demo_estimated_length_mm: 1.2,
            length_percent_diagonal: 2.0,
            mean_width_px: width_mm / DEMO_MM_PER_PIXEL,
            demo_mean_width_mm: width_mm,
            mean_width_percent_short_side: 0.1,
            dominant_orientation_deg: 0.0,
            orientation_from_horizontal_deg: 0.0,
            orientation_label: "mostly horizontal",
            foreground_pixels: 10,
            demo_scale_mm_per_px: DEMO_MM_PER_PIXEL,
        }
    }

    /// Verifies simulated severity bands and their explicit marker.
    #[test]
    fn demo_severity_boundaries() {
        assert_eq!(demo_grade(&metrics_with_width(0.29)).label, "hairline");
        assert_eq!(demo_grade(&metrics_with_width(0.30)).label, "moderate");
        assert_eq!(demo_grade(&metrics_with_width(1.00)).label, "wide");
        assert!(demo_grade(&metrics_with_width(1.00)).simulated);
    }
}
