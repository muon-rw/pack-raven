use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::commands::AppState;
use crate::setup::{self, InstanceInfo, SetupOptions};

type CmdResult<T> = Result<T, String>;

fn err(error: anyhow::Error) -> String {
    format!("{error:#}")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackList {
    active: Option<PathBuf>,
    packs: Vec<InstanceInfo>,
    instances_root: PathBuf,
}

#[tauri::command]
pub fn list_packs(state: State<'_, AppState>) -> PackList {
    let settings = state.settings();
    let packs = settings.packs.iter().filter_map(|dir| setup::inspect(dir, &settings.packs).ok()).collect();
    let active = settings.packs.iter().find(|p| setup::same_dir(p, &settings.instance_dir)).cloned();
    PackList { active, packs, instances_root: settings.instances_root() }
}

#[tauri::command]
pub fn discover_instances(state: State<'_, AppState>, root: Option<PathBuf>) -> Vec<InstanceInfo> {
    let settings = state.settings();
    setup::discover(&root.unwrap_or_else(|| settings.instances_root()), &settings.packs)
}

#[tauri::command]
pub fn inspect_instance(state: State<'_, AppState>, dir: PathBuf) -> CmdResult<InstanceInfo> {
    setup::inspect(&dir, &state.settings().packs).map_err(err)
}

#[tauri::command]
pub fn setup_instance(app: AppHandle, state: State<'_, AppState>, dir: PathBuf, options: SetupOptions) -> CmdResult<()> {
    let finished = app.clone();
    let identity = state.settings().identity();
    state.jobs.start(&app, "setup", move |log| async move {
        let message = setup::setup(dir.clone(), options, &identity, log).await?;
        use tauri::Manager;
        let state = finished.state::<AppState>();
        state.update_settings(|s| s.activate(&dir))?;
        state.rewatch(&finished);
        Ok(message)
    })
}

#[tauri::command]
pub async fn switch_pack(app: AppHandle, state: State<'_, AppState>, dir: PathBuf, kill_tests: bool) -> CmdResult<()> {
    if kill_tests {
        state.test.kill_all().await;
    }
    if state.test.running() {
        return Err("Stop the test server and client before switching packs.".into());
    }
    state.update_settings(|s| s.activate(&dir)).map_err(err)?;
    state.rewatch(&app);
    Ok(())
}

#[tauri::command]
pub fn remove_pack(app: AppHandle, state: State<'_, AppState>, dir: PathBuf) -> CmdResult<()> {
    state.update_settings(|s| s.remove(&dir)).map_err(err)?;
    state.rewatch(&app);
    Ok(())
}

#[tauri::command]
pub fn update_hooks(state: State<'_, AppState>) -> CmdResult<()> {
    setup::install_hooks(&state.settings().instance_dir).map_err(err)
}

#[tauri::command]
pub fn get_pack_java(state: State<'_, AppState>) -> Option<PathBuf> {
    let settings = state.settings();
    settings.pack_java(&settings.instance_dir).cloned()
}

#[tauri::command]
pub fn set_pack_java(state: State<'_, AppState>, java: Option<PathBuf>) -> CmdResult<()> {
    if let Some(java) = java.as_ref().filter(|java| !java.is_file()) {
        return Err(format!("{} isn't a file", java.display()));
    }
    state
        .update_settings(|s| {
            let pack = s.instance_dir.clone();
            s.set_pack_java(&pack, java);
        })
        .map_err(err)
}
