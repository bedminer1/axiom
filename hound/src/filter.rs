use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{env, path::PathBuf, time::Duration};
use tokio::{fs, process::Command, time::timeout};

use crate::deepseek::DeepSeek;

async fn command(
    python: &PathBuf,
    filter_dir: &PathBuf,
    model: &DeepSeek,
    args: &[&str],
) -> Result<()> {
    let mut process = Command::new(python);
    process
        .arg("-m")
        .arg("pfi_filter")
        .args(args)
        .current_dir(filter_dir)
        .kill_on_drop(true)
        .envs(model.filter_env());
    let output = timeout(Duration::from_secs(210), process.output())
        .await
        .context("filter timed out")??;
    ensure!(
        output.status.success() || output.status.code() == Some(2),
        "filter failed: {}",
        String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(300)
            .collect::<String>()
    );
    Ok(())
}

pub async fn analyze(project: &Value, slot: &str, image: &[u8], model: &DeepSeek) -> Result<Value> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let filter_dir = env::var_os("PFI_FILTER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("vendor/pfi-filter"));
    let python = env::var_os("PFI_FILTER_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(".venv/bin/python"));
    ensure!(
        filter_dir.join("pfi_filter/__main__.py").exists(),
        "filter source is missing"
    );
    ensure!(python.exists(), "filter Python environment is missing");

    let work = tempfile::tempdir()?;
    let input = work.path().join("in");
    let output = work.path().join("out");
    fs::create_dir_all(&input).await?;
    let photo = work.path().join("photo.jpg");
    fs::write(&photo, image).await?;
    let hound = work.path().join("hound.json");
    fs::write(&hound, serde_json::to_vec(project)?).await?;
    let hound_path = hound.to_str().context("invalid project path")?;
    let input_path = input.to_str().context("invalid input path")?;
    command(
        &python,
        &filter_dir,
        model,
        &["hound-import", "--hound", hound_path, "--out", input_path],
    )
    .await?;

    let (section, elevation) = slot.split_once(':').context("invalid photo slot")?;
    let label = match elevation {
        "North" => "N",
        "East" => "E",
        "South" => "S",
        "West" => "W",
        _ => anyhow::bail!("invalid elevation"),
    };
    let record = json!({
        "image_id": "uploaded-photo", "uri": photo, "sha256": null, "is_original": true,
        "source": {"type": "folder", "caption": null, "context_msgs": []},
        "user_labels": {"elevation": label, "level_from": null, "level_to": null,
                        "zone": null, "phase": if section == "close" {Some("close_range")} else {None}, "finding": null}
    });
    let images = work.path().join("images.jsonl");
    fs::write(&images, format!("{}\n", record)).await?;
    let images_path = images.to_str().context("invalid image path")?;
    let project_path = input.join("project.json");
    let checklist_path = input.join("checklist.json");
    let project_path = project_path.to_str().context("invalid project path")?;
    let checklist_path = checklist_path.to_str().context("invalid checklist path")?;
    let output_path = output.to_str().context("invalid output path")?;
    let mut args = vec![
        "run",
        "--images",
        images_path,
        "--project",
        project_path,
        "--checklist",
        checklist_path,
        "--out",
        output_path,
    ];
    if env::var("PFI_FILTER_OFFLINE").as_deref() == Ok("1") {
        args.push("--offline");
    }
    command(&python, &filter_dir, model, &args).await?;

    let assessment: Value = serde_json::from_str(
        fs::read_to_string(output.join("assessments.jsonl"))
            .await?
            .lines()
            .next()
            .context("filter wrote no assessment")?,
    )?;
    let slots: Value = serde_json::from_slice(&fs::read(output.join("hound_slots.json")).await?)?;
    let matched = &slots["slots"][slot];
    Ok(json!({
        "analysis": assessment["draft_observation"]["text"].as_str().unwrap_or(""),
        "source": "pfi-filter",
        "filter": {
            "decision": assessment["decision"],
            "decision_conf": assessment["decision_conf"],
            "reasons": assessment["reasons"],
            "quality": assessment["quality"],
            "shot_type": assessment["shot_type"],
            "location": assessment["location"],
            "defects": assessment["defects"],
            "question": assessment["question"],
            "match": matched,
            "summary": slots["summary"]
        }
    }))
}
