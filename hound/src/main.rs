mod deepseek;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use deepseek::DeepSeek;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::net::TcpListener;

#[derive(Clone)]
struct AppState {
    model: DeepSeek,
}

struct ApiError(anyhow::Error);

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = if self.0.to_string().contains("DeepSeek") {
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let state = Arc::new(AppState {
        model: DeepSeek::from_environment()?,
    });
    let app = Router::new()
        .route(
            "/api/health",
            get(|| async { Json(json!({"status": "ok"})) }),
        )
        .route("/api/photo/analyze", post(analyze_photo))
        .with_state(state);
    let address = std::env::var("HOUND_API_ADDRESS").unwrap_or_else(|_| "127.0.0.1:5050".into());
    let listener = TcpListener::bind(&address).await?;
    println!("Hound photo API listening on {address}");
    axum::serve(listener, app).await?;
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
    Ok(Json(
        json!({"analysis": state.model.analyze_photo(&request.slot, &request.image).await?}),
    ))
}
