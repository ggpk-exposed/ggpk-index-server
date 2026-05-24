use crate::index::state::IndexState;
use axum::{routing::get, Router};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::sync::RwLock;

mod index;
mod routes;

#[tokio::main]
async fn main() {
    let index_dir = std::env::var("INDEX_DIR").ok().map(std::path::PathBuf::from);
    let build_index = std::env::var("BUILD_INDEX").is_ok();
    let read_only = std::env::var("READ_ONLY").is_ok() || build_index;

    let state = if let Some(dir) = index_dir.as_ref() {
        if build_index {
            AppState::create(dir.clone())
        } else {
            AppState::open(dir.clone())
        }
    } else {
        AppState::new()
    };

    if build_index {
        let dir = index_dir.expect("INDEX_DIR is required for BUILD_INDEX");
        let mut urls = Vec::new();
        index::updater::try_check_urls("patch.pathofexile.com:12995", &mut urls)
            .await
            .expect("Failed to fetch PoE1 URLs");
        index::updater::try_check_urls("patch.pathofexile2.com:13060", &mut urls)
            .await
            .expect("Failed to fetch PoE2 URLs");

        println!("Building index for URLs: {:?}", urls);
        let mut writer = state
            .index
            .index
            .writer::<tantivy::TantivyDocument>(100_000_000)
            .expect("Failed to create writer");
        for url in &urls {
            index::ggpk::index(url, &writer, &state.index.fields)
                .await
                .expect("Failed to index");
        }
        writer.commit().expect("Failed to commit");

        std::fs::write(dir.join("urls.json"), serde_json::to_string(&urls).unwrap())
            .expect("Failed to save URLs");

        println!("Index build complete");
        return;
    }

    if !read_only {
        tokio::spawn(index::updater::watch(state.clone()));
    }

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

    fn open(path: std::path::PathBuf) -> Self {
        let urls_path = path.join("urls.json");
        let urls = if urls_path.exists() {
            let content = std::fs::read_to_string(urls_path).expect("Failed to read URLs");
            serde_json::from_str(&content).expect("Failed to parse URLs")
        } else {
            Vec::<String>::new()
        };
        let urls = Arc::new(RwLock::new(urls));
        let index = Box::leak(Box::new(IndexState::open(path)));
        Self { urls, index }
    }

    fn create(path: std::path::PathBuf) -> Self {
        let urls = Arc::new(RwLock::new(Vec::<String>::new()));
        let index = Box::leak(Box::new(IndexState::create(path)));
        Self { urls, index }
    }
}
