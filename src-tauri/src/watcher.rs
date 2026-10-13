use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

const IGNORED_ROOTS: [&str; 11] = [".git", "saves", "logs", "crash-reports", "screenshots", "local", "backups", "cache", "debug", ".mixin.out", "downloads"];
const IGNORED_PREFIXES: [&str; 2] = ["journeymap/data/", "config/spark/"];
const BATCH_WINDOW: Duration = Duration::from_millis(250);
const MAX_BATCH: Duration = Duration::from_secs(1);

pub struct PackWatcher {
    _watcher: RecommendedWatcher,
}

#[derive(Serialize, Clone)]
struct FsChange {
    paths: Vec<String>,
}

fn relevant(relative: &str) -> bool {
    let first = relative.split('/').next().unwrap_or_default();
    !IGNORED_ROOTS.contains(&first) && !IGNORED_PREFIXES.iter().any(|p| relative.starts_with(p)) && !relative.ends_with(".packraven-tmp")
}

fn relative(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/");
    (!relative.is_empty()).then_some(relative)
}

pub fn watch(app: AppHandle, root: PathBuf) -> Result<PackWatcher> {
    let (sender, receiver) = mpsc::channel::<String>();
    let event_root = root.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        for path in event.paths {
            if let Some(relative) = relative(&event_root, &path).filter(|r| relevant(r)) {
                let _ = sender.send(relative);
            }
        }
    })?;
    watcher.watch(&root, RecursiveMode::Recursive)?;
    std::thread::spawn(move || {
        while let Ok(first) = receiver.recv() {
            let started = Instant::now();
            let mut batch = BTreeSet::from([first]);
            while started.elapsed() < MAX_BATCH
                && let Ok(path) = receiver.recv_timeout(BATCH_WINDOW)
            {
                batch.insert(path);
            }
            let _ = app.emit("fs-change", FsChange { paths: batch.into_iter().collect() });
        }
    });
    Ok(PackWatcher { _watcher: watcher })
}

#[cfg(test)]
mod tests {
    use super::relevant;

    #[test]
    fn skips_noisy_folders() {
        assert!(relevant("config/fml.toml"));
        assert!(relevant("kubejs/server_scripts/main.js"));
        assert!(!relevant("saves/New World/level.dat"));
        assert!(!relevant(".git/index"));
        assert!(!relevant("logs/latest.log"));
        assert!(!relevant("journeymap/data/sp/x"));
        assert!(relevant("journeymap/config/6.0/journeymap.core.config"));
        assert!(!relevant("config/a.toml.packraven-tmp"));
    }
}
