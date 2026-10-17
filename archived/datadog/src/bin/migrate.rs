use anyhow::{Context, Result};
use axiom_datadog::{project_dir, project_path, store::Store};
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    let env_file = project_dir().join(".env");
    if env_file.exists() {
        dotenvy::from_path(env_file).context("cannot read datadog/.env")?;
    }
    let store = Store::open(&project_path(
        env::var("BOT_DATA_DIR").unwrap_or_else(|_| "data".into()),
    ))
    .await?;
    println!(
        "DataDog migrations applied using {}.",
        store.db.backend_name()
    );
    Ok(())
}
