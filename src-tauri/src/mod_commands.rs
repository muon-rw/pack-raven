use std::collections::HashSet;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::commands::AppState;
use crate::history::{self, Action};
use crate::mod_updates::{self, AddonKind, Category, InstalledMod, ModCheck, ModFile, ModRequest, SearchPage, SearchRequest};
use crate::modsync;
use crate::pack::{Pack, ReleaseType};
use crate::sides::MetadataCache;

type CmdResult<T> = Result<T, String>;

const CLIENT_HAS_MODS_OPEN: &str = "packraven can't change mods while the test client has them open. Close the client first.";

fn err(error: anyhow::Error) -> String {
    format!("{error:#}")
}

fn client_running(state: &AppState) -> bool {
    state.test.client.lock().unwrap().is_some()
}

#[tauri::command]
pub async fn installed_mods(state: State<'_, AppState>) -> CmdResult<Vec<InstalledMod>> {
    let pack = state.pack().map_err(err)?;
    let mut cache = MetadataCache::load(&state.cache_path());
    let mods = mod_updates::installed(&pack, &mut cache).await;
    cache.save(&state.cache_path()).map_err(err)?;
    Ok(mods)
}

#[tauri::command]
pub async fn check_mod_updates(state: State<'_, AppState>) -> CmdResult<Vec<ModCheck>> {
    mod_updates::check(&state.pack().map_err(err)?).await.map_err(err)
}

#[tauri::command]
pub async fn mod_versions(state: State<'_, AppState>, path: String, project_id: u64) -> CmdResult<Vec<ModFile>> {
    mod_updates::versions(&state.pack().map_err(err)?, &path, project_id).await.map_err(err)
}

#[tauri::command]
pub fn set_update_channel(state: State<'_, AppState>, channel: ReleaseType) -> CmdResult<()> {
    let mut pack = state.pack().map_err(err)?;
    pack.config.updates.channel = channel;
    pack.save_config().map_err(err)
}

#[tauri::command]
pub async fn search_mods(state: State<'_, AppState>, request: SearchRequest) -> CmdResult<SearchPage> {
    mod_updates::search(&state.pack().map_err(err)?, &request).await.map_err(err)
}

#[tauri::command]
pub async fn mod_categories(kind: AddonKind) -> CmdResult<Vec<Category>> {
    mod_updates::categories(kind).await.map_err(err)
}

#[derive(Serialize)]
pub struct ModPreview {
    dependencies: Vec<String>,
    unavailable: Vec<String>,
}

#[tauri::command]
pub async fn preview_mods(state: State<'_, AppState>, requests: Vec<ModRequest>) -> CmdResult<ModPreview> {
    let plan = mod_updates::plan(&state.pack().map_err(err)?, &requests).await.map_err(err)?;
    Ok(ModPreview { dependencies: plan.dependencies, unavailable: plan.unavailable })
}

#[tauri::command]
pub fn change_mods(app: AppHandle, state: State<'_, AppState>, requests: Vec<ModRequest>) -> CmdResult<()> {
    if client_running(&state) {
        return Err(CLIENT_HAS_MODS_OPEN.into());
    }
    let pack = state.pack().map_err(err)?;
    state.jobs.start(&app, "mods", move |log| async move {
        let plan = mod_updates::plan(&pack, &requests).await?;
        let updated = requests.iter().filter(|r| r.replaces.is_some()).count();
        let mut message = Vec::new();
        match updated {
            0 => {}
            1 => message.push("Updated 1 mod.".to_string()),
            n => message.push(format!("Updated {n} mods.")),
        }
        if !plan.installs.is_empty() {
            message.push(format!("Installed {}.", names(&plan.installs)));
        }
        if !plan.dependencies.is_empty() {
            message.push(format!("Added {} as required.", names(&plan.dependencies)));
        }
        if !plan.unavailable.is_empty() {
            message.push(format!("CurseForge has no file of {} for this pack's version, so you'll need to find one.", names(&plan.unavailable)));
        }
        {
            let _guard = modsync::MOD_LIST_GUARD.lock().await;
            modsync::place(&pack.dir, plan.placements, &log).await?;
        }
        Ok(message.join(" "))
    })
}

fn names(list: &[String]) -> String {
    match list {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[tauri::command]
pub async fn remove_mods(state: State<'_, AppState>, paths: Vec<String>) -> CmdResult<()> {
    if client_running(&state) {
        return Err(CLIENT_HAS_MODS_OPEN.into());
    }
    let root = state.settings().instance_dir;
    let _guard = modsync::MOD_LIST_GUARD.lock().await;
    let label = format!("Delete {}", history::describe(&paths));
    let (task_root, task_paths) = (root.clone(), paths.clone());
    let trashed = tauri::async_runtime::spawn_blocking(move || crate::files::trash(&task_root, &task_paths)).await.map_err(|e| e.to_string())?.map_err(err)?;
    state.history.lock().unwrap().record(&root, label, trashed.map(Action::Restore));
    modsync::drop_from_lock(&root, &paths).map_err(err)?;
    forget_overrides(&root).map_err(err)
}

// Server side overrides name projects, so we drop the ones whose last file you just removed.
fn forget_overrides(root: &std::path::Path) -> anyhow::Result<()> {
    let mut pack = Pack::open(root)?;
    let present: HashSet<u64> = pack.lock.files.iter().map(|e| e.project_id).collect();
    let server = &mut pack.config.server;
    let before = server.client_only.len() + server.keep.len();
    server.client_only.retain(|id| present.contains(id));
    server.keep.retain(|id| present.contains(id));
    if server.client_only.len() + server.keep.len() != before {
        pack.save_config()?;
    }
    Ok(())
}
