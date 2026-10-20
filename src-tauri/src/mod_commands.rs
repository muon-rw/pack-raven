use std::collections::HashSet;

use anyhow::Context;
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::commands::AppState;
use crate::curseforge;
use crate::history::{self, Action};
use crate::jobs::Log;
use crate::mod_updates::{self, AddonKind, Category, InstalledMods, ModCheck, ModFile, ModRequest, SearchPage, SearchRequest};
use crate::modsync;
use crate::pack::{Pack, ReleaseType, BCC_PROJECT_ID};
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
pub async fn installed_mods(state: State<'_, AppState>) -> CmdResult<InstalledMods> {
    let mut pack = state.pack().map_err(err)?;
    crate::instance_locks::sync(&mut pack, &state.curseforge_locks_path()).map_err(err)?;
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
    projects: Vec<u64>,
}

#[tauri::command]
pub async fn preview_mods(state: State<'_, AppState>, requests: Vec<ModRequest>) -> CmdResult<ModPreview> {
    let plan = mod_updates::plan(&state.pack().map_err(err)?, &requests).await.map_err(err)?;
    let projects = plan.placements.iter().map(|p| p.entry.project_id).collect();
    Ok(ModPreview { dependencies: plan.dependencies, unavailable: plan.unavailable, projects })
}

#[tauri::command]
pub fn set_mod_locked(state: State<'_, AppState>, project_id: u64, locked: bool) -> CmdResult<()> {
    let mut pack = state.pack().map_err(err)?;
    let list = &mut pack.config.updates.locked;
    list.retain(|id| *id != project_id);
    if locked {
        list.push(project_id);
        list.sort_unstable();
    }
    pack.save_config().map_err(err)?;
    crate::instance_locks::sync(&mut pack, &state.curseforge_locks_path()).map_err(err)
}

#[tauri::command]
pub async fn mod_description(project_id: u64) -> CmdResult<String> {
    crate::curseforge::description(project_id).await.map_err(err)
}

const DISABLED_SUFFIX: &str = ".disabled";

#[tauri::command]
pub async fn mod_changelog(project_id: u64, file_id: u64) -> CmdResult<String> {
    crate::curseforge::changelog(project_id, file_id).await.map_err(err)
}

#[tauri::command]
pub async fn set_mods_enabled(state: State<'_, AppState>, paths: Vec<String>, enabled: bool) -> CmdResult<Vec<String>> {
    if client_running(&state) {
        return Err(CLIENT_HAS_MODS_OPEN.into());
    }
    let dir = state.settings().instance_dir;
    let _guard = modsync::MOD_LIST_GUARD.lock().await;
    let mut renamed_paths = Vec::new();
    for path in paths {
        let Some((folder, _)) = path.split_once('/').filter(|(folder, _)| ["mods", "resourcepacks", "shaderpacks", "datapacks"].contains(folder)) else {
            return Err(format!("{path} isn't in a folder packraven manages"));
        };
        let renamed = match (enabled, path.strip_suffix(DISABLED_SUFFIX)) {
            (true, Some(plain)) => plain.to_string(),
            (false, None) => format!("{path}{DISABLED_SUFFIX}"),
            _ => {
                renamed_paths.push(path);
                continue;
            }
        };
        // Packs set up before we supported disabling resource packs and shaders don't ignore their
        // disabled files yet, and git would offer to commit them.
        if !enabled {
            crate::setup::ensure_lines(&dir.join(".gitignore"), &[&format!("{folder}/*{DISABLED_SUFFIX}")]).map_err(err)?;
        }
        modsync::rename_locked_file(&dir, &path, &renamed).map_err(err)?;
        renamed_paths.push(renamed);
    }
    Ok(renamed_paths)
}

pub struct ModChange {
    requests: Vec<ModRequest>,
    updated: Vec<String>,
}

#[tauri::command]
pub fn change_mods(app: AppHandle, state: State<'_, AppState>, requests: Vec<ModRequest>) -> CmdResult<u64> {
    // The client holds the jars it loaded open, so we can add files while it runs but not replace them.
    if client_running(&state) && requests.iter().any(|r| r.replaces.is_some()) {
        return Err(CLIENT_HAS_MODS_OPEN.into());
    }
    let pack = state.pack().map_err(err)?;
    let cache = MetadataCache::load(&state.cache_path());
    let updated: Vec<String> = requests
        .iter()
        .filter_map(|r| r.replaces.as_deref())
        .map(|path| {
            let project = pack.lock.files.iter().find(|e| e.path == path).map(|e| e.project_id);
            project.and_then(|id| cache.known_name(id)).map_or_else(|| path.rsplit('/').next().unwrap_or(path).to_string(), |n| n.name.clone())
        })
        .collect();
    let dir = pack.dir;
    state.jobs.enqueue(&app, "mods", &state.mod_changes, ModChange { requests, updated }, move |change, log| {
        let dir = dir.clone();
        async move {
            // An earlier change in the same run may have added files, so we read the pack again.
            let pack = Pack::open(&dir)?;
            apply_mod_requests(&pack, &change.requests, &change.updated, &log).await
        }
    })
}

// Once a change starts, stopping it partway could leave two versions of a mod in the folder, so
// you can only cancel the ones still waiting.
#[tauri::command]
pub fn cancel_mod_change(state: State<'_, AppState>, id: u64) -> bool {
    state.mod_changes.lock().unwrap().cancel(id)
}

async fn apply_mod_requests(pack: &Pack, requests: &[ModRequest], updated: &[String], log: &Log) -> anyhow::Result<String> {
    let plan = mod_updates::plan(pack, requests).await?;
    let mut message = Vec::new();
    // Past a handful of names, a count reads better than a list.
    match updated.len() {
        0 => {}
        1..=4 => message.push(format!("Updated {}.", names(updated))),
        n => message.push(format!("Updated {}, {} and {} more.", updated[0], updated[1], n - 2)),
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
        modsync::place(&pack.dir, plan.placements, log).await?;
    }
    Ok(message.join(" "))
}

#[tauri::command]
pub fn set_up_version_checker(app: AppHandle, state: State<'_, AppState>, version: String) -> CmdResult<()> {
    let pack = state.pack().map_err(err)?;
    if pack.has_bcc() {
        return pack.write_bcc_config_if_missing(version.trim()).map_err(err);
    }
    if client_running(&state) {
        return Err(CLIENT_HAS_MODS_OPEN.into());
    }
    state.jobs.start(&app, "mods", move |log| async move {
        let loader = curseforge::loader_type(&pack.lock.mod_loader()?);
        let files = curseforge::project_files(BCC_PROJECT_ID, pack.lock.minecraft()?, Some(loader)).await?;
        let file = files
            .iter()
            .find(|f| f.release_type == 1)
            .or(files.first())
            .context("CurseForge has no Better Compatibility Checker file for this pack's Minecraft version and loader")?;
        let message = apply_mod_requests(&pack, &[ModRequest { file_id: file.id, replaces: None }], &[], &log).await?;
        pack.write_bcc_config_if_missing(version.trim())?;
        Ok(message)
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
    forget_removed_projects(&root).map_err(err)
}

// Overrides and locks name projects, so we drop the ones whose last file you just removed.
fn forget_removed_projects(root: &std::path::Path) -> anyhow::Result<()> {
    let mut pack = Pack::open(root)?;
    let present: HashSet<u64> = pack.lock.files.iter().map(|e| e.project_id).collect();
    let lists = [&mut pack.config.server.client_only, &mut pack.config.server.keep, &mut pack.config.updates.locked];
    let mut changed = false;
    for list in lists {
        let before = list.len();
        list.retain(|id| present.contains(id));
        changed |= list.len() != before;
    }
    if changed {
        pack.save_config()?;
    }
    Ok(())
}
