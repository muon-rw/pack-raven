use std::path::PathBuf;

use serde::Serialize;
use tauri::State;

use crate::commands::AppState;
use crate::credentials::{self, Credential, CredentialStatus};
use crate::defender;
use crate::setup::same_dir;

type CmdResult<T> = Result<T, String>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefenderView {
    active: bool,
    exclusions_readable: bool,
    missing: Vec<PathBuf>,
    prompt: bool,
}

fn watched_paths(state: &AppState) -> Vec<PathBuf> {
    let settings = state.settings();
    [settings.instance_dir, settings.install_dir, state.data_dir()].into_iter().filter(|p| !p.as_os_str().is_empty()).collect()
}

#[tauri::command]
pub async fn defender_status(state: State<'_, AppState>) -> CmdResult<DefenderView> {
    let paths = watched_paths(&state);
    let settings = state.settings();
    let status = tauri::async_runtime::spawn_blocking(move || defender::status(&paths)).await.map_err(|e| e.to_string())?;
    let missing: Vec<PathBuf> = if status.exclusions_readable {
        status.missing
    } else {
        status.missing.into_iter().filter(|p| !settings.defender_excluded.iter().any(|e| same_dir(e, p))).collect()
    };
    let prompt = status.active && !missing.is_empty() && !settings.defender_dismissed;
    Ok(DefenderView { active: status.active, exclusions_readable: status.exclusions_readable, missing, prompt })
}

#[tauri::command]
pub async fn defender_exclude(state: State<'_, AppState>, paths: Vec<PathBuf>) -> CmdResult<()> {
    let to_add = paths.clone();
    tauri::async_runtime::spawn_blocking(move || defender::exclude(&to_add)).await.map_err(|e| e.to_string())?.map_err(|e| format!("{e:#}"))?;
    state
        .update_settings(|s| {
            for path in paths {
                if !s.defender_excluded.iter().any(|e| same_dir(e, &path)) {
                    s.defender_excluded.push(path);
                }
            }
        })
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn defender_dismiss(state: State<'_, AppState>, dismissed: bool) -> CmdResult<()> {
    state.update_settings(|s| s.defender_dismissed = dismissed).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn git_version() -> Option<String> {
    crate::git::version()
}

#[tauri::command]
pub async fn install_git() -> CmdResult<String> {
    crate::git_install::install().await.map_err(|e| format!("{e:#}"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsView {
    download: CredentialStatus,
    upload: CredentialStatus,
    keychain: &'static str,
}

#[tauri::command]
pub fn credentials_status() -> CredentialsView {
    CredentialsView { download: credentials::status(Credential::Download), upload: credentials::status(Credential::Upload), keychain: credentials::keychain_name() }
}

#[tauri::command]
pub fn credential_store(credential: Credential, secret: String) -> CmdResult<()> {
    credentials::store(credential, &secret).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn credential_use_env(credential: Credential, name: String) -> CmdResult<()> {
    credentials::use_env(credential, &name).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn credential_clear(credential: Credential) -> CmdResult<()> {
    credentials::clear(credential).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn env_var_names() -> Vec<String> {
    credentials::env_names()
}
