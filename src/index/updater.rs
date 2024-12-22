use crate::index::state::IndexState;
use crate::AppState;
use std::io::ErrorKind;
use std::time::Duration;
use tantivy::TantivyDocument;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub async fn watch(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs(10 * 60));
    loop {
        interval.tick().await;
        let mut updated = Vec::with_capacity(2);
        if check_urls("patch.pathofexile.com:12995", &mut updated).await
            && check_urls("patch.pathofexile2.com:13060", &mut updated).await
        {
            let prev = { state.urls.read().await.clone() };
            let removed = prev
                .iter()
                .filter(|url| !updated.contains(url))
                .cloned()
                .collect::<Vec<_>>();
            let added = updated
                .iter()
                .filter(|url| !prev.contains(url))
                .cloned()
                .collect::<Vec<_>>();
            if (!removed.is_empty() || !added.is_empty())
                && reindex(state.index, removed, added).await
            {
                *state.urls.write().await = updated;
            }
        }
    }
}

async fn reindex(state: &'static IndexState, removed: Vec<String>, added: Vec<String>) -> bool {
    match tokio::task::spawn_blocking(move || try_reindex(state, removed, added)).await {
        Ok(Ok(())) => true,
        Ok(Err(e)) => {
            eprintln!("Error building index {}", e);
            false
        }
        Err(e) => {
            eprintln!("Join error on reindex task {}", e);
            false
        }
    }
}

fn try_reindex(
    IndexState { index, fields, .. }: &IndexState,
    removed: Vec<String>,
    added: Vec<String>,
) -> anyhow::Result<()> {
    let mut writer = index.writer::<TantivyDocument>(50_000_000)?;
    for r in &removed {
        writer.delete_term(fields.version_term(r.as_str()));
    }
    for r in &added {
        crate::index::ggpk::index(r.as_str(), &writer, fields)?
    }
    writer.commit()?;
    println!("Index updated - added {:?}, removed {:?}", added, removed);
    Ok(())
}

async fn check_urls(addr: &'static str, out: &mut Vec<String>) -> bool {
    let result = tokio::time::timeout(Duration::from_secs(10), try_check_urls(addr, out)).await;
    match result {
        Err(_) => {
            eprintln!("Timed out connecting to {}", addr);
            false
        }
        Ok(Err(e)) => {
            eprintln!("Error getting urls from {}: {:?}", addr, e);
            false
        }
        Ok(Ok(())) => true,
    }
}

async fn try_check_urls(addr: &'static str, out: &mut Vec<String>) -> Result<(), std::io::Error> {
    let mut stream = TcpStream::connect(addr).await?;
    stream.write_all(&[1, 7]).await?;
    let mut buf = [0; 1000];
    let read = stream.read(&mut buf).await?;
    if read < 34 {
        return Err(std::io::Error::new(
            ErrorKind::InvalidData,
            format!("Server returned only {} bytes", read),
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
                format!("len {} too big", len),
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