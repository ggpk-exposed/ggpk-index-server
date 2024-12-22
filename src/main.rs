use crate::index::state::IndexState;
use axum::{routing::get, Router};
use std::net::{Ipv4Addr, SocketAddr};
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

    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let addr = SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), port);
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
