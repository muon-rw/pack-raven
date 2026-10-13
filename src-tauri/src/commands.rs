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
use crate::sync;
use crate::testrun::{self, TestRun};

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub settings_path: PathBuf,
    pub data_dir: PathBuf,
    pub jobs: Jobs,
    pub test: Arc<TestRun>,
    pub watcher: Mutex<Option<crate::watcher::PackWatcher>>,
}

impl AppState {
    pub(crate) fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    pub(crate) fn rewatch(&self, app: &AppHandle) {
        let dir = self.settings().instance_dir;
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

    fn pack(&self) -> Result<Pack> {
        Pack::open(&self.settings().instance_dir)
    }

    pub(crate) fn pack_data(&self) -> PathBuf {
        pack_data_dir(&self.data_dir, &self.settings().instance_dir)
    }

    fn cache_path(&self) -> PathBuf {
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
    missing: Vec<String>,
    unlocked: Vec<String>,
    branch: Option<String>,
    ahead: u32,
    behind: u32,
    changed: Vec<String>,
    last_tag: Option<String>,
    dev_jars: Vec<String>,
    unpublishable_jars: Vec<String>,
    project_id: u64,
    has_core_key: bool,
    has_upload_token: bool,
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

#[tauri::command]
pub fn overview(state: State<'_, AppState>) -> CmdResult<Overview> {
    let pack = state.pack().map_err(err)?;
    let sync = sync::status(&pack).map_err(err)?;
    let (ahead, behind) = git::ahead_behind(&pack.dir).unwrap_or((0, 0));
    Ok(Overview {
        name: pack.name(),
        version: pack.version(),
        minecraft: pack.lock.minecraft.clone(),
        loader: pack.lock.loader.clone(),
        lock_count: pack.lock.files.len(),
        addons: addon_counts(&pack),
        missing: sync.missing,
        unlocked: sync.unlocked,
        branch: git::branch(&pack.dir),
        ahead,
        behind,
        changed: git::changed_files(&pack.dir),
        last_tag: git::last_release_tag(&pack.dir),
        dev_jars: export::local_dev_jars(&pack),
        unpublishable_jars: export::unallowed_override_jars(&pack).unwrap_or_default(),
        project_id: pack.config.curseforge.project_id,
        has_core_key: curseforge::core_key().is_some(),
        has_upload_token: curseforge::upload_token().is_some(),
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
    let mut cache = MetadataCache::load(&state.cache_path());
    let diff = changelog::mod_diff(&pack, &mut cache).await.map_err(err)?;
    cache.save(&state.cache_path()).map_err(err)?;
    let markdown = changelog::render(&changelog::read_notes(&pack), &diff);
    Ok(ChangelogPreview { diff, markdown })
}

#[tauri::command]
pub async fn server_sides(state: State<'_, AppState>) -> CmdResult<Vec<ModSide>> {
    let pack = state.pack().map_err(err)?;
    let mut cache = MetadataCache::load(&state.cache_path());
    let result = sides::classify(&pack, &mut cache).await.map_err(err)?;
    cache.save(&state.cache_path()).map_err(err)?;
    Ok(result)
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

#[tauri::command]
pub fn publish_preflight(state: State<'_, AppState>, version: String) -> CmdResult<Vec<String>> {
    Ok(publish::preflight(&state.pack().map_err(err)?, version.trim()))
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

async fn build_server(state_data: (PathBuf, PathBuf, PathBuf), pack: &Pack, java: &std::path::Path, log: &crate::jobs::Log) -> Result<PathBuf> {
    let (data_dir, pack_data, cache_path) = state_data;
    let version = pack.version().unwrap_or_else(|| "dev".into());
    let mut cache = MetadataCache::load(&cache_path);
    let sides = sides::classify(pack, &mut cache).await?;
    cache.save(&cache_path)?;
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

#[tauri::command]
pub fn start_test(app: AppHandle, state: State<'_, AppState>, rebuild: bool, connect: bool) -> CmdResult<()> {
    if state.test.server.lock().unwrap().is_some() {
        return Err("the test server is already running".into());
    }
    let settings = state.settings();
    let pack = state.pack().map_err(err)?;
    let java = launch::java(&settings).map_err(err)?;
    let data = (state.data_dir.clone(), state.pack_data(), state.cache_path());
    let test_dir = state.pack_data().join("test-server");
    let natives = state.pack_data().join("natives");
    let jobs = state.jobs.clone();
    let run = state.test.clone();
    let job_app = app.clone();
    state.jobs.start(&app, "test", move |log| async move {
        let version = pack.version().unwrap_or_else(|| "dev".into());
        let built = data.1.join("builds").join(&version).join("server");
        if rebuild || !built.exists() {
            build_server(data, &pack, &java, &log).await?;
        }
        log.info("Syncing the test server folder");
        testrun::sync_server_dir(&built, &test_dir)?;
        let dev_jars = testrun::add_dev_jars(&pack, &test_dir)?;
        if !dev_jars.is_empty() {
            log.info(format!("Added local dev builds: {}", dev_jars.join(", ")));
        }
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
