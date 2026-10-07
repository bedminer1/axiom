//! Offline smoke check: cargo run --example detect -- INPUT_IMAGE OUTPUT_JPEG
use anyhow::{Context, Result};
use axiom_telebot::{default_runtime_path, detector::Detector, project_path};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    anyhow::ensure!(args.len() == 2, "usage: detect INPUT_IMAGE OUTPUT_JPEG");
    let mut detector = Detector::load(
        &project_path("models/crack.onnx"),
        &default_runtime_path(),
        0.35,
        0.45,
    )?;
    let input = std::fs::read(&args[0]).context("cannot read input image")?;
    let processed = detector.process(&input)?;
    std::fs::write(&args[1], &processed.jpeg)?;
    println!(
        "{} candidates, {} inference passes, {} JPEG bytes",
        processed.detections,
        processed.passes,
        processed.jpeg.len()
    );
    Ok(())
}
