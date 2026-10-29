use std::sync::Mutex;
use std::time::Duration;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::commands::AppState;
use crate::credentials::{self, Credential};

const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const RELEASES_API: &str = "https://api.github.com/repos/muon-rw/packraven/releases?per_page=100";
pub const CHANGED_EVENT: &str = "self-update-changed";
// Dev builds would compare against the latest release and replace themselves with it.
const SUPPORTED: bool = !cfg!(debug_assertions);

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseNotes {
    pub version: String,
    pub published_at: Option<String>,
    pub body: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    published_at: Option<String>,
    body: Option<String>,
}

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
    /// The version we just updated from, until you dismiss the changelog window.
    updated_from: Option<String>,
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
    let current = app.package_info().version.to_string();
    let settings = app.state::<AppState>().settings();
    UpdateView {
        updated_from: settings.updated_from.filter(|from| is_newer(&current, from)),
        current,
        auto: settings.auto_update,
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
    record_updating_from(app);
    update.install(bytes).map_err(|e| {
        clear_updating_from(app);
        failed(app, Stage::Install, e.into())
    })?;
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
    record_updating_from(app);
    if let Err(error) = update.restart_after_install(false).install(bytes) {
        clear_updating_from(app);
        eprintln!("packraven: installing {version} on exit failed: {error:#}");
    }
}

fn record_updating_from(app: &AppHandle) {
    let current = app.package_info().version.to_string();
    let _ = app.state::<AppState>().update_settings(|s| {
        // Keep an earlier version if you haven't dismissed its changelog before updating again.
        if !s.updated_from.as_deref().is_some_and(|prev| is_newer(&current, prev)) {
            s.updated_from = Some(current);
        }
    });
}

fn clear_updating_from(app: &AppHandle) {
    let _ = app.state::<AppState>().update_settings(|s| s.updated_from = None);
}

pub fn dismiss_updated_from(app: &AppHandle) -> Result<()> {
    app.state::<AppState>().update_settings(|s| s.updated_from = None)?;
    let _ = app.emit(CHANGED_EVENT, view(app));
    Ok(())
}

pub async fn changelog(app: &AppHandle, since: Option<&str>, through: Option<&str>) -> Result<Vec<ReleaseNotes>> {
    let current = app.package_info().version.to_string();
    let since = since.unwrap_or(&current);
    let through = through
        .map(String::from)
        .or_else(|| app.state::<SelfUpdate>().pending.lock().unwrap().as_ref().map(|p| p.update.version.clone()))
        .unwrap_or_else(|| current.clone());
    let pending_fallback = app.state::<SelfUpdate>().pending.lock().unwrap().as_ref().and_then(|p| {
        let body = p.update.body.as_deref()?.trim();
        (!body.is_empty()).then(|| ReleaseNotes {
            version: p.update.version.clone(),
            published_at: p.update.date.and_then(|d| chrono::DateTime::from_timestamp(d.unix_timestamp(), 0).map(|dt| dt.format("%Y-%m-%d").to_string())),
            body: body.to_string(),
        })
    });
    match fetch_github_releases().await {
        Ok(releases) => {
            let notes = filter_releases(releases, since, &through);
            if notes.is_empty()
                && let Some(fallback) = pending_fallback.filter(|f| f.version == through)
            {
                return Ok(vec![fallback]);
            }
            Ok(notes)
        }
        Err(error) => {
            if let Some(fallback) = pending_fallback.filter(|f| f.version == through) {
                return Ok(vec![fallback]);
            }
            Err(error)
        }
    }
}

async fn fetch_github_releases() -> Result<Vec<GithubRelease>> {
    let mut request = crate::http::CLIENT.get(RELEASES_API).header("Accept", "application/vnd.github+json");
    if let Some(token) = credentials::get(Credential::Github) {
        request = request.bearer_auth(token);
    }
    let response = request.send().await?;
    // An expired token shouldn't block reading public release notes, so retry without it.
    let response = if matches!(response.status().as_u16(), 401 | 403) {
        crate::http::CLIENT.get(RELEASES_API).header("Accept", "application/vnd.github+json").send().await?
    } else {
        response
    };
    Ok(response.error_for_status()?.json().await?)
}

fn parse_semver(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.trim().strip_prefix('v').unwrap_or(version.trim()).split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.split(['-', '+']).next()?.parse().ok()?;
    Some((major, minor, patch))
}

fn is_newer(a: &str, b: &str) -> bool {
    match (parse_semver(a), parse_semver(b)) {
        (Some(a), Some(b)) => a > b,
        _ => a != b,
    }
}

fn filter_releases(releases: Vec<GithubRelease>, since: &str, through: &str) -> Vec<ReleaseNotes> {
    let since_v = parse_semver(since);
    let through_v = parse_semver(through);
    let mut matching: Vec<(Option<(u64, u64, u64)>, ReleaseNotes)> = releases
        .into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| {
            let version = r.tag_name.strip_prefix('v').unwrap_or(&r.tag_name).to_string();
            let parsed = parse_semver(&version);
            let in_range = match (parsed, since_v, through_v) {
                (Some(v), Some(s), Some(t)) if s < t => v > s && v <= t,
                (Some(v), _, Some(t)) => v == t,
                _ => version == through,
            };
            if !in_range {
                return None;
            }
            let body = r.body.unwrap_or_default().trim().to_string();
            let published_at = r.published_at.as_deref().and_then(|d| d.get(..10)).map(String::from);
            Some((parsed, ReleaseNotes { version, published_at, body }))
        })
        .collect();
    matching.sort_by(|(a, _), (b, _)| b.cmp(a));
    matching.into_iter().map(|(_, notes)| notes).collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, date: &str, body: &str, draft: bool) -> GithubRelease {
        GithubRelease { tag_name: tag.into(), draft, published_at: Some(format!("{date}T12:00:00Z")), body: Some(body.into()) }
    }

    #[test]
    fn filters_and_orders_skipped_versions_newest_first() {
        let releases = vec![
            release("v0.1.0", "2026-10-19", "Initial", false),
            release("v0.1.1", "2026-10-20", "Queued updates", false),
            release("v0.2.0", "2026-10-28", "Sign in from Run", false),
            release("v0.2.1", "2026-10-28", "Lock format", false),
            release("v0.3.0", "2026-10-29", "Modrinth", true),
        ];
        let notes = filter_releases(releases, "0.1.0", "0.2.1");
        assert_eq!(notes.iter().map(|n| n.version.as_str()).collect::<Vec<_>>(), ["0.2.1", "0.2.0", "0.1.1"]);
        assert_eq!(notes[0].published_at.as_deref(), Some("2026-10-28"));

        let single = filter_releases(vec![release("v0.2.1", "2026-10-28", "Lock format", false)], "0.2.1", "0.2.1");
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].version, "0.2.1");
    }
}
