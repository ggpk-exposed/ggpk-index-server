use crate::AppState;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Params {
    poe: usize,
}

pub async fn handler(
    Query(Params { poe }): Query<Params>,
    State(state): State<AppState>,
) -> String {
    if let [poe1, poe2] = { state.urls.read().await.clone().as_slice() } {
        if poe == 1 {
            poe1.clone()
        } else {
            poe2.clone()
        }
    } else {
        String::default()
    }
}

pub async fn socket_handler(
    Query(Params { poe }): Query<Params>,
) -> String {
    let addr = if poe == 1 {
        "patch.pathofexile.com:12995"
    } else {
        "patch.pathofexile2.com:13060"
    };
    let mut urls = Vec::new();
    if crate::index::updater::try_check_urls(addr, &mut urls).await.is_ok() {
        urls.into_iter().next().unwrap_or_default()
    } else {
        String::default()
    }
}
