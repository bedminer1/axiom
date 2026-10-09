//! Axiom executable entry point.
//!
//! Configures logging, resolves the repository root, and delegates HTTP
//! application construction to the server module.

mod domain;
mod geometry;
mod ml;
mod server;

use anyhow::Result;

/// Starts the Axiom HTTP server with repository-relative assets and workers.
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "axiom=info,tower_http=info".into()),
        )
        .init();
    let root = std::env::current_dir()?.canonicalize()?;
    server::serve(root).await
}
