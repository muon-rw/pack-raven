use std::path::PathBuf;

use serde::Serialize;
use tauri::State;

use crate::commands::AppState;
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
