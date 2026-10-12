use std::path::Path;
use std::sync::LazyLock;

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use sha1::{Digest, Sha1};
use tokio::io::AsyncWriteExt;

pub static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(concat!("muon-rw/packraven/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
});

pub async fn download(url: &str, dest: &Path, sha1: Option<&str>) -> Result<()> {
    let response = CLIENT.get(url).send().await?.error_for_status().with_context(|| format!("downloading {url}"))?;
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let temp = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&temp).await?;
    let mut hasher = Sha1::new();
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);
    if let Some(expected) = sha1 {
        let actual = hex::encode(hasher.finalize());
        if actual != expected {
            tokio::fs::remove_file(&temp).await?;
            bail!("hash mismatch for {url}: expected {expected}, got {actual}");
        }
    }
    tokio::fs::rename(&temp, dest).await?;
    Ok(())
}
