use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use serde::Serialize;
use tauri::{AppHandle, State};
use tokio::sync::watch;

use crate::changelog::{self, ModDiff};
use crate::curseforge;
use crate::export;
use crate::git;
use crate::jobs::Jobs;
use crate::launch;
use crate::pack::{Pack, PackConfig};
use crate::publish::{self, PublishContext, PublishOptions};
use crate::settings::Settings;
use crate::sides::{self, MetadataCache, ModSide};
use crate::modsync::{self, ModCounts};
use crate::testrun::{self, TestRun};

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub settings_path: PathBuf,
    pub data_dir: PathBuf,
    pub jobs: Jobs,
    pub test: Arc<TestRun>,
    pub watcher: Mutex<Option<crate::watcher::PackWatcher>>,
    pub history: Mutex<crate::history::History>,
    pub mod_list: Mutex<crate::mod_list::ModListState>,
}

impl AppState {
    pub(crate) fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    pub(crate) fn rewatch(&self, app: &AppHandle) {
        let dir = self.settings().instance_dir;
        if crate::setup::has_hooks(&dir) {
            let _ = crate::setup::link_hooks(&dir);
        }
        let watcher = (!dir.as_os_str().is_empty()).then(|| crate::watcher::watch(app.clone(), dir).ok()).flatten();
        *self.watcher.lock().unwrap() = watcher;
    }

    pub(crate) fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }

    pub(crate) fn update_settings(&self, change: impl FnOnce(&mut Settings)) -> Result<()> {
        let mut settings = self.settings.lock().unwrap();
        change(&mut settings);
        settings.save(&self.settings_path)
    }

    pub(crate) fn pack(&self) -> Result<Pack> {
        Pack::open(&self.settings().instance_dir)
    }

    pub(crate) fn pack_data(&self) -> PathBuf {
        pack_data_dir(&self.data_dir, &self.settings().instance_dir)
    }

    pub(crate) fn cache_path(&self) -> PathBuf {
        self.data_dir.join("metadata-cache.json")
    }

    fn version(&self, pack: &Pack) -> String {
        pack.version().unwrap_or_else(|| "dev".into())
    }
}

pub fn pack_data_dir(data_dir: &std::path::Path, instance_dir: &std::path::Path) -> PathBuf {
    let name = instance_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let slug: String = name.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    data_dir.join("packs").join(slug)
}

type CmdResult<T> = Result<T, String>;

fn err(error: anyhow::Error) -> String {
    format!("{error:#}")
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> CmdResult<()> {
    if let Some((major, java)) = settings.java_by_major.iter().find(|(_, java)| !java.is_file()) {
        return Err(format!("The Java {major} path, {}, isn't a file", java.display()));
    }
    settings.save(&state.settings_path).map_err(err)?;
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    name: String,
    version: Option<String>,
    minecraft: Option<String>,
    loader: Option<String>,
    lock_count: usize,
    addons: AddonCounts,
    not_on_curseforge: Vec<String>,
    mod_list_error: Option<String>,
    branch: Option<String>,
    ahead: u32,
    behind: u32,
    incoming: Option<ModCounts>,
    changed: Vec<String>,
    last_tag: Option<String>,
    dev_jars: Vec<String>,
    unpublishable_jars: Vec<String>,
    project_id: u64,
    running_jobs: Vec<String>,
    server_running: bool,
    client_running: bool,
    hooks: crate::setup::HooksState,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AddonCounts {
    mods: usize,
    resource_packs: usize,
    shader_packs: usize,
    data_packs: usize,
    other: usize,
}

fn addon_counts(pack: &Pack) -> AddonCounts {
    let mut counts = AddonCounts::default();
    for entry in &pack.lock.files {
        let slot = match entry.path.split('/').next().unwrap_or_default() {
            "mods" => &mut counts.mods,
            "resourcepacks" => &mut counts.resource_packs,
            "shaderpacks" => &mut counts.shader_packs,
            _ if entry.path.contains("datapacks/") => &mut counts.data_packs,
            _ => &mut counts.other,
        };
        *slot += 1;
    }
    counts
}

fn incoming_mods(dir: &std::path::Path) -> Option<ModCounts> {
    Some(modsync::mod_counts(&modsync::lock_at(dir, "HEAD").ok()?, &modsync::lock_at(dir, "@{upstream}").ok()?))
}

// The shell polls this every few seconds, and it runs git, so we keep it off the main thread.
#[tauri::command(async)]
pub fn overview(state: State<'_, AppState>) -> CmdResult<Overview> {
    let pack = state.pack().map_err(err)?;
    let mod_list = state.mod_list.lock().unwrap().clone();
    let mod_list = if crate::setup::same_dir(&mod_list.dir, &pack.dir) { mod_list } else { Default::default() };
    let (ahead, behind) = git::ahead_behind(&pack.dir).unwrap_or((0, 0));
    Ok(Overview {
        name: pack.name(),
        version: pack.version(),
        minecraft: pack.lock.minecraft.clone(),
        loader: pack.lock.loader.clone(),
        lock_count: pack.lock.files.len(),
        addons: addon_counts(&pack),
        not_on_curseforge: mod_list.not_on_curseforge,
        mod_list_error: mod_list.error,
        branch: git::branch(&pack.dir),
        ahead,
        behind,
        incoming: if behind > 0 { incoming_mods(&pack.dir) } else { None },
        changed: git::changed_files(&pack.dir),
        last_tag: git::last_release_tag(&pack.dir),
        dev_jars: export::local_dev_jars(&pack),
        unpublishable_jars: export::unallowed_override_jars(&pack).unwrap_or_default(),
        project_id: pack.config.curseforge.project_id,
        running_jobs: state.jobs.running(),
        server_running: state.test.server.lock().unwrap().is_some(),
        client_running: state.test.client.lock().unwrap().is_some(),
        hooks: crate::setup::inspect(&pack.dir, &[]).map(|i| i.hooks).unwrap_or(crate::setup::HooksState::Missing),
    })
}

#[tauri::command]
pub fn get_pack_config(state: State<'_, AppState>) -> CmdResult<PackConfig> {
    Ok(state.pack().map_err(err)?.config)
}

#[tauri::command]
pub fn save_pack_config(state: State<'_, AppState>, config: PackConfig) -> CmdResult<()> {
    let mut pack = state.pack().map_err(err)?;
    pack.config = config;
    pack.save_config().map_err(err)
}

#[tauri::command]
pub fn set_version(state: State<'_, AppState>, version: String) -> CmdResult<()> {
    state.pack().map_err(err)?.set_version(version.trim()).map_err(err)
}

#[tauri::command]
pub fn get_notes(state: State<'_, AppState>) -> CmdResult<String> {
    Ok(changelog::read_notes(&state.pack().map_err(err)?))
}

#[tauri::command]
pub fn save_notes(state: State<'_, AppState>, text: String) -> CmdResult<()> {
    changelog::write_notes(&state.pack().map_err(err)?, &text).map_err(err)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangelogPreview {
    diff: ModDiff,
    markdown: String,
}

#[tauri::command]
pub async fn changelog_preview(state: State<'_, AppState>) -> CmdResult<ChangelogPreview> {
    let pack = state.pack().map_err(err)?;
    // We diff the mods folder rather than the mod list, so the preview already shows changes you
    // haven't recorded yet. If the folder can't be read as a lock, we fall back to the list.
    let folder = modsync::folder_lock(&pack.dir).await.unwrap_or_else(|_| pack.lock.clone());
    let mut cache = MetadataCache::load(&state.cache_path());
    let diff = changelog::mod_diff(&pack, &folder, &mut cache, &state.pack_data().join(changelog::BASELINE_FILE)).await.map_err(err)?;
    cache.save(&state.cache_path()).map_err(err)?;
    let markdown = changelog::render(&changelog::read_notes(&pack), &diff);
    Ok(ChangelogPreview { diff, markdown })
}

#[tauri::command]
pub async fn track_directly(app: AppHandle, state: State<'_, AppState>, paths: Vec<String>) -> CmdResult<()> {
    let dir = state.settings().instance_dir;
    modsync::track_directly(&dir, &paths).map_err(err)?;
    crate::mod_list::refresh_now(&app, &dir).await;
    Ok(())
}

#[tauri::command]
pub fn revert_mods(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let dir = state.settings().instance_dir;
    // The restore future borrows lock entries across its download stream, which the compiler can't
    // prove Send, so we drive it on a blocking thread the way the git hook's CLI does.
    state.jobs.start(&app, "restore", move |log| async move {
        tauri::async_runtime::spawn_blocking(move || {
            let _guard = modsync::MOD_LIST_GUARD.blocking_lock();
            git::run(&dir, &["checkout", "HEAD", "--", crate::pack::LOCKFILE])?;
            tauri::async_runtime::block_on(modsync::restore(&dir, &log))
        })
        .await??;
        Ok("Your mods match the last commit again.".into())
    })
}

#[tauri::command]
pub fn set_compare_baseline(state: State<'_, AppState>, zip: String) -> CmdResult<()> {
    let baseline = changelog::baseline_from_zip(std::path::Path::new(&zip)).map_err(err)?;
    changelog::save_baseline(&state.pack_data().join(changelog::BASELINE_FILE), &baseline).map_err(err)
}

#[tauri::command]
pub fn clear_compare_baseline(state: State<'_, AppState>) -> CmdResult<()> {
    changelog::clear_baseline(&state.pack_data().join(changelog::BASELINE_FILE)).map_err(err)
}

#[tauri::command]
pub async fn server_sides(state: State<'_, AppState>) -> CmdResult<Vec<ModSide>> {
    let pack = state.pack().map_err(err)?;
    classify_cached(&state.cache_path(), &pack).await.map_err(err)
}

#[tauri::command]
pub fn set_side_override(state: State<'_, AppState>, project_id: u64, mode: String) -> CmdResult<()> {
    let mut pack = state.pack().map_err(err)?;
    let server = &mut pack.config.server;
    server.client_only.retain(|id| *id != project_id);
    server.keep.retain(|id| *id != project_id);
    match mode.as_str() {
        "client" => server.client_only.push(project_id),
        "keep" => server.keep.push(project_id),
        _ => {}
    }
    server.client_only.sort_unstable();
    server.keep.sort_unstable();
    pack.save_config().map_err(err)
}

#[tauri::command(async)]
pub fn publish_preflight(state: State<'_, AppState>, version: String) -> CmdResult<Vec<String>> {
    let not_on_curseforge = state.mod_list.lock().unwrap().not_on_curseforge.clone();
    Ok(publish::preflight(&state.pack().map_err(err)?, version.trim(), &not_on_curseforge, &state.settings().identity()))
}

#[tauri::command]
pub fn build_client_export(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let pack = state.pack().map_err(err)?;
    let out_dir = state.pack_data().join("builds").join(state.version(&pack));
    state.jobs.start(&app, "client-export", move |log| async move {
        let path = export::client_export(&pack, &pack.version().unwrap_or_else(|| "dev".into()), &out_dir, &log)?;
        Ok(path.to_string_lossy().into_owned())
    })
}

async fn classify_cached(cache_path: &std::path::Path, pack: &Pack) -> Result<Vec<ModSide>> {
    let mut cache = MetadataCache::load(cache_path);
    let sides = sides::classify(pack, &mut cache).await?;
    cache.save(cache_path)?;
    Ok(sides)
}

async fn build_server(state_data: (PathBuf, PathBuf, PathBuf), pack: &Pack, java: &std::path::Path, log: &crate::jobs::Log) -> Result<PathBuf> {
    let (data_dir, pack_data, cache_path) = state_data;
    let version = pack.version().unwrap_or_else(|| "dev".into());
    let sides = classify_cached(&cache_path, pack).await?;
    export::server_pack(pack, &sides, &version, &pack_data.join("builds").join(&version), java, &data_dir, log).await
}

#[tauri::command]
pub fn build_server_pack(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let pack = state.pack().map_err(err)?;
    let java = launch::java(&state.settings()).map_err(err)?;
    let data = (state.data_dir.clone(), state.pack_data(), state.cache_path());
    state.jobs.start(&app, "server-pack", move |log| async move {
        let dir = build_server(data, &pack, &java, &log).await?;
        let version = pack.version().unwrap_or_else(|| "dev".into());
        let out_dir = dir.parent().context("server pack folder has no parent")?;
        Ok(export::zip_server_pack(&pack, &version, out_dir, &log).await?.to_string_lossy().into_owned())
    })
}

const MODS_CHANGING: &str = "packraven is changing this pack's mods. Wait for it to finish, then try again.";

fn mods_changing(state: &AppState) -> bool {
    state.jobs.running().iter().any(|job| job == "mods")
}

#[tauri::command]
pub fn start_test(app: AppHandle, state: State<'_, AppState>, recopy_all: bool, connect: bool) -> CmdResult<()> {
    if state.test.server.lock().unwrap().is_some() {
        return Err("the test server is already running".into());
    }
    if mods_changing(&state) {
        return Err(MODS_CHANGING.into());
    }
    let settings = state.settings();
    let pack = state.pack().map_err(err)?;
    let java = launch::java(&settings).map_err(err)?;
    let (data_dir, cache_path) = (state.data_dir.clone(), state.cache_path());
    let test_dir = state.pack_data().join("test-server");
    let natives = state.pack_data().join("natives");
    let jobs = state.jobs.clone();
    let run = state.test.clone();
    let job_app = app.clone();
    state.jobs.start(&app, "test", move |log| async move {
        let version = pack.version().unwrap_or_else(|| "dev".into());
        let sides = classify_cached(&cache_path, &pack).await?;
        let plan = export::server_plan(&pack, &sides, &version, &java, &data_dir, &log).await?;
        log.info(if recopy_all { "Recopying every file in the test server folder" } else { "Syncing changed files to the test server folder" });
        let report = testrun::sync_server_dir(plan, &pack, &test_dir, recopy_all, &log)?;
        log.info(report.to_string());
        let (ready_tx, mut ready_rx) = watch::channel(false);
        let server_pack = Pack::open(&pack.dir)?;
        let (server_app, server_run) = (job_app.clone(), run.clone());
        jobs.start(&job_app, "server", move |server_log| async move {
            testrun::run_server(server_app, server_run, &server_pack, java, test_dir, ready_tx, server_log).await
        })
        .map_err(anyhow::Error::msg)?;
        if !connect {
            return Ok("Server starting".into());
        }
        log.info("Waiting for the server to finish starting");
        while !*ready_rx.borrow() {
            if ready_rx.changed().await.is_err() {
                anyhow::bail!("the server stopped before it finished starting");
            }
        }
        let client_pack = Pack::open(&pack.dir)?;
        let client_app = job_app.clone();
        jobs.start(&job_app, "client", move |client_log| async move {
            testrun::run_client(client_app, run, &settings, &client_pack, natives, true, client_log).await
        })
        .map_err(anyhow::Error::msg)?;
        Ok("Server ready, client launching".into())
    })
}

#[tauri::command]
pub fn start_client(app: AppHandle, state: State<'_, AppState>, join: bool) -> CmdResult<()> {
    if mods_changing(&state) {
        return Err(MODS_CHANGING.into());
    }
    let settings = state.settings();
    let pack = state.pack().map_err(err)?;
    let natives = state.pack_data().join("natives");
    let run = state.test.clone();
    let job_app = app.clone();
    state.jobs.start(&app, "client", move |log| async move { testrun::run_client(job_app, run, &settings, &pack, natives, join, log).await })
}

#[tauri::command]
pub async fn server_command(state: State<'_, AppState>, line: String) -> CmdResult<()> {
    let server = state.test.server.lock().unwrap().clone().ok_or("the server is not running")?;
    server.send(&line).await.map_err(err)
}

#[tauri::command]
pub fn kill_process(state: State<'_, AppState>, which: String) -> CmdResult<()> {
    let slot = if which == "server" { &state.test.server } else { &state.test.client };
    if let Some(process) = slot.lock().unwrap().as_ref() {
        process.kill();
    }
    Ok(())
}

#[tauri::command]
pub fn publish(app: AppHandle, state: State<'_, AppState>, options: PublishOptions) -> CmdResult<()> {
    let pack = state.pack().map_err(err)?;
    let announce = pack.config.discord.announce_on_approval && state.settings().webhook(&pack.dir).is_some();
    let context = PublishContext {
        announce,
        pack,
        identity: state.settings().identity(),
        java: launch::java(&state.settings()).map_err(err)?,
        data_dir: state.data_dir.clone(),
        pack_data: state.pack_data(),
        cache_path: state.cache_path(),
    };
    state.jobs.start(&app, "publish", move |log| publish::publish(context, options, log))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseStatus {
    #[serde(flatten)]
    release: publish::Release,
    status: String,
    server_status: String,
    server_pack_linked: Option<bool>,
}

#[tauri::command]
pub async fn releases(state: State<'_, AppState>) -> CmdResult<Vec<ReleaseStatus>> {
    let mut result = Vec::new();
    for release in publish::load_releases(&state.pack_data().join("releases.json")).into_iter().rev() {
        let file = curseforge::file(release.project_id, release.file_id).await.ok().flatten();
        let server = curseforge::file(release.project_id, release.server_file_id).await.ok().flatten();
        let describe = |f: &Option<curseforge::CfFile>| f.as_ref().map(|f| curseforge::file_status_name(f.file_status).to_string()).unwrap_or_else(|| "Not public yet".into());
        result.push(ReleaseStatus {
            status: describe(&file),
            server_status: describe(&server),
            server_pack_linked: file.as_ref().and_then(|f| f.server_pack_file_id).map(|id| id == release.server_file_id),
            release,
        });
    }
    Ok(result)
}

#[tauri::command]
pub fn output_dir(state: State<'_, AppState>) -> String {
    state.pack_data().join("builds").to_string_lossy().into_owned()
}
