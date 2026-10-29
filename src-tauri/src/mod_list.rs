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

#[derive(Default, Clone, PartialEq)]
pub struct ModListState {
    pub dir: PathBuf,
    pub unhosted: Vec<String>,
    pub modrinth_error: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize, Clone)]
struct ModListChanged;

// Listeners reload the whole list, so we only tell them when the lockfile or what we report changed.
pub fn record(app: &AppHandle, dir: &Path, result: Result<modsync::Refresh>) {
    let (state, lock_changed) = match result {
        Ok(refresh) => (ModListState { dir: dir.to_path_buf(), unhosted: refresh.unknown, modrinth_error: refresh.modrinth_error, error: None }, refresh.changed),
        Err(error) => (ModListState { dir: dir.to_path_buf(), error: Some(format!("{error:#}")), ..Default::default() }, false),
    };
    let app_state = app.state::<AppState>();
    if !crate::setup::same_dir(dir, &app_state.settings().instance_dir) {
        return;
    }
    let state_changed = {
        let mut current = app_state.mod_list.lock().unwrap();
        let changed = *current != state;
        *current = state;
        changed
    };
    if lock_changed || state_changed {
        let _ = app.emit("mod-list-changed", ModListChanged);
    }
}

pub async fn refresh_now(app: &AppHandle, dir: &Path, lookup: modsync::Lookup) {
    let _guard = modsync::MOD_LIST_GUARD.lock().await;
    record(app, dir, modsync::refresh(dir, lookup).await);
}

// The watcher can miss changes (notify drops them on Windows when its buffer overflows), so we also
// resync when you come back to packraven or check for updates. During a git hook's restore we leave
// the folder to the watcher's refresh, which waits for the restore to finish.
pub async fn resync(app: &AppHandle, dir: &Path, lookup: modsync::Lookup) {
    if !modsync::restore_in_progress(dir) {
        refresh_now(app, dir, lookup).await;
    }
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
            record(&app, &root, tauri::async_runtime::block_on(modsync::refresh(&root, modsync::Lookup::Cached)));
        }
    });
    let _ = sender.send(());
    sender
}
