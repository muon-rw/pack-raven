use serde::Serialize;
use tauri::{AppHandle, State};

use crate::commands::AppState;
use crate::gitops::{self, Commit, Diff, Status};

type CmdResult<T> = Result<T, String>;

fn err(error: anyhow::Error) -> String {
    format!("{error:#}")
}

#[derive(Serialize)]
pub struct GitOverview {
    status: Status,
    commits: Vec<Commit>,
}

#[tauri::command]
pub fn git_overview(state: State<'_, AppState>) -> CmdResult<GitOverview> {
    let dir = state.settings().instance_dir;
    Ok(GitOverview { status: gitops::status(&dir).map_err(err)?, commits: gitops::recent_commits(&dir, 30) })
}

#[tauri::command]
pub fn git_diff(state: State<'_, AppState>, path: String, staged: bool, untracked: bool) -> CmdResult<Diff> {
    gitops::diff(&state.settings().instance_dir, &path, staged, untracked).map_err(err)
}

#[tauri::command]
pub fn git_stage(state: State<'_, AppState>, paths: Vec<String>) -> CmdResult<()> {
    gitops::stage(&state.settings().instance_dir, &paths).map_err(err)
}

#[tauri::command]
pub fn git_unstage(state: State<'_, AppState>, paths: Vec<String>) -> CmdResult<()> {
    gitops::unstage(&state.settings().instance_dir, &paths).map_err(err)
}

#[tauri::command]
pub fn git_discard(state: State<'_, AppState>, tracked: Vec<String>, untracked: Vec<String>) -> CmdResult<()> {
    gitops::discard(&state.settings().instance_dir, &tracked, &untracked).map_err(err)
}

#[tauri::command]
pub fn git_commit(app: AppHandle, state: State<'_, AppState>, message: String) -> CmdResult<()> {
    let dir = state.settings().instance_dir;
    state.jobs.start(&app, "git", move |log| async move { gitops::commit(&dir, &message, &log).await })
}

#[tauri::command]
pub fn git_sync(app: AppHandle, state: State<'_, AppState>, action: String) -> CmdResult<()> {
    let dir = state.settings().instance_dir;
    state.jobs.start(&app, "git", move |log| async move {
        match action.as_str() {
            "pull" => gitops::pull(&dir, &log).await,
            "push" => gitops::push(&dir, &log).await,
            _ => gitops::fetch(&dir, &log).await,
        }
    })
}
