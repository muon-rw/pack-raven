use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::AppState;
use crate::modsync;

// The CurseForge app writes a mod's jar in several steps, and a restore pauses between removing
// files and placing downloads, so we wait for the folder to settle before we read it.
const QUIET_PERIOD: Duration = Duration::from_secs(3);

#[derive(Default, Clone)]
pub struct ModListState {
    pub dir: PathBuf,
    pub not_on_curseforge: Vec<String>,
    pub error: Option<String>,
}

#[derive(Serialize, Clone)]
struct ModListChanged;

pub fn record(app: &AppHandle, dir: &Path, result: Result<Vec<String>>) {
    let state = match result {
        Ok(unknown) => ModListState { dir: dir.to_path_buf(), not_on_curseforge: unknown, error: None },
        Err(error) => ModListState { dir: dir.to_path_buf(), not_on_curseforge: Vec::new(), error: Some(format!("{error:#}")) },
    };
    *app.state::<AppState>().mod_list.lock().unwrap() = state;
    let _ = app.emit("mod-list-changed", ModListChanged);
}

pub async fn refresh_now(app: &AppHandle, dir: &Path) {
    let _guard = modsync::MOD_LIST_GUARD.lock().await;
    record(app, dir, modsync::refresh_mod_list(dir).await);
}

/// Starts the thread that keeps mods.lock.json matching the mods folder. Send on the returned
/// channel when the folder changes; dropping it stops the thread.
pub fn spawn_updater(app: AppHandle, root: PathBuf) -> mpsc::Sender<()> {
    let (sender, receiver) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        while receiver.recv().is_ok() {
            while receiver.recv_timeout(QUIET_PERIOD).is_ok() {}
            while modsync::restore_in_progress(&root) {
                std::thread::sleep(QUIET_PERIOD);
            }
            let _guard = modsync::MOD_LIST_GUARD.blocking_lock();
            record(&app, &root, tauri::async_runtime::block_on(modsync::refresh_mod_list(&root)));
        }
    });
    let _ = sender.send(());
    sender
}
