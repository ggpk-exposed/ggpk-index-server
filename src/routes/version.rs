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
