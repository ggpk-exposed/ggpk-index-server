use crate::index::state::IndexState;
use axum::{routing::get, Router};
use std::sync::Arc;
use tokio::sync::RwLock;

mod index;
mod routes;

#[tokio::main]
async fn main() {
    let state = AppState::new();

    tokio::spawn(index::updater::watch(state.clone()));
    let app = Router::new()
        .route("/files", get(routes::browse::handler))
        .route("/version", get(routes::version::handler))
        .with_state(state);

    let addr = std::env::var("PORT").map_or_else(
        |_| "0.0.0.0:3000".to_string(),
        |port| format!("0.0.0.0:{}", port),
    );

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[derive(Clone)]
pub struct AppState {
    pub urls: Arc<RwLock<Vec<String>>>,
    pub index: &'static IndexState,
}

impl AppState {
    fn new() -> Self {
        let urls = Arc::new(RwLock::new(Vec::<String>::new()));
        let index = Box::leak(Box::new(IndexState::new()));
        Self { urls, index }
    }
}
