use std::sync::Mutex;
use std::time::Duration;

use anyhow::{bail, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::commands::AppState;

const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
pub const CHANGED_EVENT: &str = "self-update-changed";
// Dev builds would compare against the latest release and replace themselves with it.
const SUPPORTED: bool = !cfg!(debug_assertions);

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    Check,
    Download,
    Install,
}

#[derive(Serialize, Clone, Default)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum Phase {
    #[default]
    Unchecked,
    Checking,
    UpToDate,
    Available { version: String },
    Downloading { version: String, percent: Option<u8> },
    Ready { version: String },
    Failed { stage: Stage, error: String },
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateView {
    current: String,
    auto: bool,
    supported: bool,
    /// Why restarting to update would be refused right now.
    blocked: Option<String>,
    status: Phase,
}

struct Pending {
    update: Update,
    bytes: Option<Vec<u8>>,
}

#[derive(Default)]
pub struct SelfUpdate {
    phase: Mutex<Phase>,
    pending: Mutex<Option<Pending>>,
    checking: tokio::sync::Mutex<()>,
}

pub fn start(app: AppHandle) {
    if !SUPPORTED {
        return;
    }
    tauri::async_runtime::spawn(async move {
        loop {
            let _ = check(&app).await;
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

pub fn view(app: &AppHandle) -> UpdateView {
    UpdateView {
        current: app.package_info().version.to_string(),
        auto: app.state::<AppState>().settings().auto_update,
        supported: SUPPORTED,
        blocked: blocked(app),
        status: app.state::<SelfUpdate>().phase.lock().unwrap().clone(),
    }
}

fn blocked(app: &AppHandle) -> Option<String> {
    let state = app.state::<AppState>();
    if state.test.server.lock().unwrap().is_some() || state.test.client.lock().unwrap().is_some() {
        return Some("Stop the test server and client to update, since restarting packraven closes them.".into());
    }
    if !state.jobs.running().is_empty() {
        return Some("Wait for packraven's running tasks to finish to update.".into());
    }
    None
}

fn set_phase(app: &AppHandle, phase: Phase) {
    *app.state::<SelfUpdate>().phase.lock().unwrap() = phase;
    let _ = app.emit(CHANGED_EVENT, view(app));
}

fn failed(app: &AppHandle, stage: Stage, error: anyhow::Error) -> anyhow::Error {
    set_phase(app, Phase::Failed { stage, error: format!("{error:#}") });
    error
}

pub async fn check(app: &AppHandle) -> Result<()> {
    if !SUPPORTED {
        bail!("Development builds don't update themselves");
    }
    let state = app.state::<SelfUpdate>();
    // A second check would only race the first one's download.
    let Ok(_checking) = state.checking.try_lock() else { return Ok(()) };
    set_phase(app, Phase::Checking);
    let found = async { anyhow::Ok(app.updater()?.check().await?) }.await.map_err(|e| failed(app, Stage::Check, e))?;
    let Some(update) = found else {
        *state.pending.lock().unwrap() = None;
        set_phase(app, Phase::UpToDate);
        return Ok(());
    };
    let version = update.version.clone();
    let already_downloaded = state.pending.lock().unwrap().as_ref().is_some_and(|p| p.update.version == version && p.bytes.is_some());
    if already_downloaded {
        set_phase(app, Phase::Ready { version });
        return Ok(());
    }
    if !app.state::<AppState>().settings().auto_update {
        *state.pending.lock().unwrap() = Some(Pending { update, bytes: None });
        set_phase(app, Phase::Available { version });
        return Ok(());
    }
    let bytes = download(app, &update).await.map_err(|e| failed(app, Stage::Download, e))?;
    *state.pending.lock().unwrap() = Some(Pending { update, bytes: Some(bytes) });
    set_phase(app, Phase::Ready { version });
    Ok(())
}

async fn download(app: &AppHandle, update: &Update) -> Result<Vec<u8>> {
    let version = update.version.clone();
    set_phase(app, Phase::Downloading { version: version.clone(), percent: None });
    let mut received = 0u64;
    let mut shown = None;
    let bytes = update
        .download(
            |chunk, total| {
                received += chunk as u64;
                let percent = total.map(|total| (received * 100 / total.max(1)).min(100) as u8);
                // Chunks arrive every few kilobytes, so we only tell the UI when the percentage moves.
                if percent != shown {
                    shown = percent;
                    set_phase(app, Phase::Downloading { version: version.clone(), percent });
                }
            },
            || {},
        )
        .await?;
    Ok(bytes)
}

/// Installs the pending update and restarts packraven into it, downloading it first if needed.
pub async fn install(app: &AppHandle) -> Result<()> {
    if let Some(reason) = blocked(app) {
        bail!(reason);
    }
    let Some(Pending { update, bytes }) = app.state::<SelfUpdate>().pending.lock().unwrap().take() else {
        bail!("There's no update to install");
    };
    let bytes = match bytes {
        Some(bytes) => bytes,
        None => download(app, &update).await.map_err(|e| failed(app, Stage::Download, e))?,
    };
    // On Windows this hands off to the installer, which exits packraven and relaunches it.
    update.install(bytes).map_err(|e| failed(app, Stage::Install, e.into()))?;
    app.restart()
}

/// Installs an update we already downloaded as packraven exits. With automatic updates on, this
/// is the only time we install without you asking, so packraven never restarts under you.
pub fn install_on_exit(app: &AppHandle) {
    if !app.state::<AppState>().settings().auto_update {
        return;
    }
    let Some(Pending { update, bytes: Some(bytes) }) = app.state::<SelfUpdate>().pending.lock().unwrap().take() else { return };
    let version = update.version.clone();
    if let Err(error) = update.restart_after_install(false).install(bytes) {
        eprintln!("packraven: installing {version} on exit failed: {error:#}");
    }
}

pub fn auto_update_changed(app: &AppHandle, enabled: bool) {
    // We download an update found while automatic updates were off once you turn them on.
    let available = matches!(*app.state::<SelfUpdate>().phase.lock().unwrap(), Phase::Available { .. });
    if enabled && available {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { check(&app).await });
    } else {
        let _ = app.emit(CHANGED_EVENT, view(app));
    }
}
