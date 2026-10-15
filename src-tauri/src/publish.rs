use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::changelog;
use crate::curseforge;
use crate::export;
use crate::git;
use crate::jobs::Log;
use crate::git::Identity;
use crate::pack::{Pack, CHANGELOG_ARCHIVE};
use crate::sides::{self, MetadataCache};
use crate::modsync;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    pub project_id: u64,
    pub file_id: u64,
    pub server_file_id: u64,
    pub published_at: String,
    #[serde(default)]
    pub announce: bool,
    #[serde(default)]
    pub announced_at: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishOptions {
    pub version: String,
    pub message: String,
    pub push: bool,
}

pub fn load_releases(path: &Path) -> Vec<Release> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_releases(path: &Path, releases: &[Release]) -> Result<()> {
    std::fs::write(path, serde_json::to_string_pretty(releases)?)?;
    Ok(())
}

pub fn preflight(pack: &Pack, version: &str, not_on_curseforge: &[String], identity: &Identity) -> Vec<String> {
    let mut problems = Vec::new();
    if pack.config.curseforge.project_id == 0 {
        problems.push("Set the CurseForge project ID in Pack settings.".into());
    }
    if curseforge::upload_token().is_none() {
        problems.push("Add a CurseForge upload token in App settings.".into());
    }
    if curseforge::core_key().is_none() {
        problems.push("Add a CurseForge API key in App settings.".into());
    }
    if version.trim().is_empty() {
        problems.push("Enter a version.".into());
    } else if git::run(&pack.dir, &["rev-parse", "-q", "--verify", &format!("refs/tags/v{version}")]).is_ok() {
        problems.push(format!("You already released {version} (tag v{version} exists)."));
    }
    match not_on_curseforge {
        [] => {}
        [path] => problems.push(format!("{path} isn't on CurseForge. In Changes, add it to Git or move it to the trash.")),
        paths => problems.push(format!("{} files aren't on CurseForge. In Changes, add them to Git or move them to the trash.", paths.len())),
    }
    // The commit comes after the upload, which you can't take back, so we check git can make it first.
    if !identity.is_usable(&pack.dir) {
        problems.push("Set your commit name and email in App settings.".into());
    }
    match export::unallowed_override_jars(pack) {
        Ok(jars) if !jars.is_empty() => problems.push(format!(
            "These jars would be in overrides, which CurseForge rejects: {}. Remove them or allow them in packraven.toml.",
            jars.join(", ")
        )),
        Err(error) => problems.push(format!("Couldn't check overrides: {error:#}")),
        _ => {}
    }
    problems
}

pub struct PublishContext {
    pub pack: Pack,
    pub identity: Identity,
    pub java: PathBuf,
    pub data_dir: PathBuf,
    pub pack_data: PathBuf,
    pub cache_path: PathBuf,
    pub announce: bool,
}

pub async fn publish(ctx: PublishContext, options: PublishOptions, log: Log) -> Result<String> {
    let PublishContext { pack, identity, java, data_dir, pack_data, cache_path, announce } = ctx;
    let version = options.version.trim().to_string();
    let _guard = modsync::MOD_LIST_GUARD.lock().await;
    let unknown = modsync::refresh_mod_list(&pack.dir).await?;
    let problems = preflight(&pack, &version, &unknown, &identity);
    if !problems.is_empty() {
        bail!("{}", problems.join("\n"));
    }
    let pack = Pack::open(&pack.dir)?;
    let project_id = pack.config.curseforge.project_id;

    pack.set_version(&version)?;
    log.info(format!("Set the pack version to {version}"));
    let out_dir = pack_data.join("builds").join(&version);
    let client_zip = export::client_export(&pack, &version, &out_dir, &log)?;

    let mut cache = MetadataCache::load(&cache_path);
    let sides = sides::classify(&pack, &mut cache).await?;
    let baseline_path = pack_data.join(changelog::BASELINE_FILE);
    let diff = changelog::mod_diff(&pack, &pack.lock, &mut cache, &baseline_path).await?;
    cache.save(&cache_path)?;
    export::server_pack(&pack, &sides, &version, &out_dir, &java, &data_dir, &log).await?;
    let server_zip = export::zip_server_pack(&pack, &version, &out_dir, &log).await?;
    let notes = changelog::render(&changelog::read_notes(&pack), &diff);

    let game_versions = curseforge::game_version_ids(pack.lock.minecraft()?, pack.lock.mod_loader()?.name()).await?;
    let release_type = pack.config.curseforge.release_type.clone();
    log.info(format!("Uploading {}", client_zip.display()));
    let file_id = curseforge::upload(
        project_id,
        &client_zip,
        json!({
            "changelog": notes,
            "changelogType": "markdown",
            "displayName": format!("{} {version}", pack.name()),
            "gameVersions": game_versions,
            "releaseType": release_type,
            "isMarkedForManualRelease": pack.config.curseforge.manual_release,
        }),
    )
    .await?;
    log.info(format!("Uploaded the client pack as file {file_id}"));
    log.info(format!("Uploading {}", server_zip.display()));
    let server_file_id = curseforge::upload(
        project_id,
        &server_zip,
        json!({
            "changelog": notes,
            "changelogType": "markdown",
            "displayName": format!("{} {version} Server Pack", pack.name()),
            "parentFileID": file_id,
            "releaseType": release_type,
        }),
    )
    .await
    .context("packraven uploaded the client pack, but the server pack upload failed; upload the server pack as an additional file on CurseForge")?;
    log.info(format!("Uploaded the server pack as file {server_file_id}"));

    let releases_path = pack_data.join("releases.json");
    let mut releases = load_releases(&releases_path);
    releases.push(Release { version: version.clone(), project_id, file_id, server_file_id, published_at: chrono::Utc::now().to_rfc3339(), announce, announced_at: None });
    save_releases(&releases_path, &releases)?;

    let archive = pack.dir.join(CHANGELOG_ARCHIVE);
    std::fs::create_dir_all(&archive)?;
    std::fs::write(archive.join(format!("{version}.md")), &notes)?;
    changelog::write_notes(&pack, "")?;
    let _ = std::fs::remove_file(&baseline_path);
    // packraven built the packs from disk, so we commit everything, and the tag records what shipped.
    git::run(&pack.dir, &["add", "-A"])?;
    let message = Some(options.message.trim()).filter(|m| !m.is_empty()).map_or_else(|| format!("Release {version}"), String::from);
    let tag = format!("v{version}");
    for command in [&["commit", "--allow-empty", "-m", &message][..], &["tag", "-a", &tag, "-m", &message]] {
        let mut args = identity.config_args();
        args.extend(command.iter().map(|a| a.to_string()));
        git::run(&pack.dir, &args.iter().map(String::as_str).collect::<Vec<_>>())?;
    }
    log.info(format!("Committed \"{message}\" and tagged v{version}"));
    if options.push {
        crate::gitops::push(&pack.dir, &log)
            .await
            .with_context(|| format!("packraven published {version} and tagged v{version}, but couldn't push. Once you've fixed this, push from Git"))?;
        log.info("Pushed the release commit and tag");
    }
    Ok(format!("Published {version}: pack file {file_id}, server pack {server_file_id}"))
}
