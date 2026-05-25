use crate::index::state::IndexState;
use crate::AppState;
use std::io::ErrorKind;
use std::sync::Arc;
use std::time::Duration;
use tantivy::TantivyDocument;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::RwLock;

pub async fn watch(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs(10 * 60));
    loop {
        interval.tick().await;

        let mut poe1_updated = Vec::with_capacity(1);
        if check_urls("patch.pathofexile.com:12995", &mut poe1_updated).await {
            let prev = { state.poe1.read().await.clone() };
            if update_storage(&state, &prev, poe1_updated, &state.poe1).await {
                println!("PoE1 updated");
            }
        }

        let mut poe2_updated = Vec::with_capacity(1);
        if check_urls("patch.pathofexile2.com:13060", &mut poe2_updated).await {
            let prev = { state.poe2.read().await.clone() };
            if update_storage(&state, &prev, poe2_updated, &state.poe2).await {
                println!("PoE2 updated");
            }
        }
    }
}

async fn update_storage(
    state: &AppState,
    prev: &[String],
    updated: Vec<String>,
    lock: &Arc<RwLock<Vec<String>>>,
) -> bool {
    let removed = subtract(prev, &updated);
    let added = subtract(&updated, prev);

    if (!removed.is_empty() || !added.is_empty())
        && reindex(state.index, removed, added).await
            .map_err(|e| eprintln!("indexing failed: {e:?}"))
            .is_ok()
        {
            let mut urls = lock.write().await;
            *urls = updated;
            return true;
        }
    false
}

fn subtract(prev: &[String], updated: &[String]) -> Vec<String> {
    prev.iter()
        .filter(|url| !updated.contains(url))
        .cloned()
        .collect::<Vec<_>>()
}

async fn reindex(
    IndexState { index, fields, .. }: &IndexState,
    removed: Vec<String>,
    added: Vec<String>,
) -> anyhow::Result<()> {
    println!("Updating index - added {added:?}, removed {removed:?}");
    let mut writer = index.writer::<TantivyDocument>(50_000_000)?;
    for r in &removed {
        writer.delete_term(fields.version_term(r.as_str()));
    }
    for r in &added {
        crate::index::ggpk::index(r.as_str(), &writer, fields).await?
    }
    writer.commit()?;
    println!("Index updated");
    Ok(())
}

async fn check_urls(addr: &'static str, out: &mut Vec<String>) -> bool {
    let result = tokio::time::timeout(Duration::from_secs(10), try_check_urls(addr, out)).await;
    match result {
        Err(_) => {
            eprintln!("Timed out connecting to {addr}");
            false
        }
        Ok(Err(e)) => {
            eprintln!("Error getting urls from {addr}: {e:?}");
            false
        }
        Ok(Ok(())) => true,
    }
}

pub async fn try_check_urls(addr: &'static str, out: &mut Vec<String>) -> Result<(), std::io::Error> {
    let mut stream = TcpStream::connect(addr).await?;
    stream.write_all(&[1, 7]).await?;
    let mut buf = [0; 1000];
    let read = stream.read(&mut buf).await?;
    if read < 34 {
        return Err(std::io::Error::new(
            ErrorKind::InvalidData,
            format!("Server returned only {read} bytes"),
        ));
    }
    let mut data = &buf[34..read];
    while !data.is_empty() {
        let len = data[0] as usize;
        data = &data[1..];
        if len == 0 {
            continue;
        } else if len > data.len() {
            return Err(std::io::Error::new(
                ErrorKind::InvalidData,
                format!("len {len} too big"),
            ));
        }
        let raw = data
            .chunks(2)
            .take(len)
            .map(|chunk| u16::from_le_bytes(chunk.try_into().unwrap()))
            .collect::<Vec<_>>();
        match String::from_utf16(&raw) {
            Ok(url) => {
                if !out.contains(&url) {
                    out.push(url.clone())
                }
            }
            Err(e) => return Err(std::io::Error::new(ErrorKind::InvalidData, e)),
        }
        data = &data[2 * len..];
    }
    Ok(())
}
