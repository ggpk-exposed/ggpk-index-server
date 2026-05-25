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
        let mut poe1_urls = Vec::new();
        index::updater::try_check_urls("patch.pathofexile.com:12995", &mut poe1_urls)
            .await
            .expect("Failed to fetch PoE1 URLs");
        let mut poe2_urls = Vec::new();
        index::updater::try_check_urls("patch.pathofexile2.com:13060", &mut poe2_urls)
            .await
            .expect("Failed to fetch PoE2 URLs");

        println!("Building index for PoE1 URLs: {poe1_urls:?}, PoE2 URLs: {poe2_urls:?}");
        let mut writer = state
            .index
            .index
            .writer::<tantivy::TantivyDocument>(100_000_000)
            .expect("Failed to create writer");
        for url in poe1_urls.iter().chain(poe2_urls.iter()) {
            index::ggpk::index(url, &writer, &state.index.fields)
                .await
                .expect("Failed to index");
        }
        writer.commit().expect("Failed to commit");

        let mut map = std::collections::HashMap::new();
        map.insert("poe1", poe1_urls);
        map.insert("poe2", poe2_urls);

        std::fs::write(dir.join("urls.json"), serde_json::to_string(&map).unwrap())
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
        .route("/check-version", get(routes::version::socket_handler))
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
    pub poe1: Arc<RwLock<Vec<String>>>,
    pub poe2: Arc<RwLock<Vec<String>>>,
    pub index: &'static IndexState,
}

impl AppState {
    fn new() -> Self {
        let poe1 = Arc::new(RwLock::new(Vec::<String>::new()));
        let poe2 = Arc::new(RwLock::new(Vec::<String>::new()));
        let index = Box::leak(Box::new(IndexState::new()));
        Self { poe1, poe2, index }
    }

    fn open(path: std::path::PathBuf) -> Self {
        let urls_path = path.join("urls.json");
        let (poe1, poe2) = if urls_path.exists() {
            let content = std::fs::read_to_string(urls_path).expect("Failed to read URLs");
            let map: std::collections::HashMap<String, Vec<String>> =
                serde_json::from_str(&content).expect("Failed to parse URLs");
            (
                map.get("poe1").cloned().unwrap_or_default(),
                map.get("poe2").cloned().unwrap_or_default(),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        let poe1 = Arc::new(RwLock::new(poe1));
        let poe2 = Arc::new(RwLock::new(poe2));
        let index = Box::leak(Box::new(IndexState::open(path)));
        Self { poe1, poe2, index }
    }

    fn create(path: std::path::PathBuf) -> Self {
        let poe1 = Arc::new(RwLock::new(Vec::<String>::new()));
        let poe2 = Arc::new(RwLock::new(Vec::<String>::new()));
        let index = Box::leak(Box::new(IndexState::create(path)));
        Self { poe1, poe2, index }
    }

    pub async fn storages(&self) -> Vec<String> {
        vec!["poe1".to_string(), "poe2".to_string()]
    }

    pub async fn urls(&self, storage: &str) -> Vec<String> {
        match storage {
            "poe1" => self.poe1.read().await.clone(),
            "poe2" => self.poe2.read().await.clone(),
            _ => Vec::new(),
        }
    }
}
