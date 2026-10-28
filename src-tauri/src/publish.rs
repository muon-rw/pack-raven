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
use crate::pack::{version_problem, Pack, CHANGELOG_ARCHIVE};
use crate::sides::{self, MetadataCache};
use crate::modsync;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", from = "StoredRelease")]
pub struct Release {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curseforge: Option<CurseforgeRelease>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modrinth: Option<ModrinthRelease>,
    pub published_at: String,
    pub announce: bool,
    pub announced_at: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CurseforgeRelease {
    pub project_id: u64,
    pub file_id: u64,
    pub server_file_id: u64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModrinthRelease {
    pub project_id: String,
    pub version_id: String,
}

// packraven 0.2.1 and earlier only published to CurseForge, and kept its IDs at the top level.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredRelease {
    version: String,
    curseforge: Option<CurseforgeRelease>,
    modrinth: Option<ModrinthRelease>,
    project_id: Option<u64>,
    file_id: Option<u64>,
    server_file_id: Option<u64>,
    published_at: String,
    #[serde(default)]
    announce: bool,
    #[serde(default)]
    announced_at: Option<String>,
}

impl From<StoredRelease> for Release {
    fn from(stored: StoredRelease) -> Self {
        let legacy = match (stored.project_id, stored.file_id) {
            (Some(project_id), Some(file_id)) => Some(CurseforgeRelease { project_id, file_id, server_file_id: stored.server_file_id.unwrap_or_default() }),
            _ => None,
        };
        Self {
            version: stored.version,
            curseforge: stored.curseforge.or(legacy),
            modrinth: stored.modrinth,
            published_at: stored.published_at,
            announce: stored.announce,
            announced_at: stored.announced_at,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishOptions {
    pub version: String,
    pub message: String,
    pub push: bool,
    #[serde(default)]
    pub client: UploadNames,
    #[serde(default)]
    pub server: UploadNames,
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct UploadNames {
    pub display_name: String,
    pub file_name: String,
}

pub fn load_releases(path: &Path) -> Vec<Release> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_releases(path: &Path, releases: &[Release]) -> Result<()> {
    std::fs::write(path, serde_json::to_string_pretty(releases)?)?;
    Ok(())
}

const VERSION_TOKEN: &str = "{version}";
// Windows rejects these in file names, and players download the packs onto Windows.
const UNSAFE_FILE_NAME_CHARS: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

pub fn default_client_names(pack: &Pack) -> UploadNames {
    UploadNames { display_name: format!("{} {VERSION_TOKEN}", pack.name()), file_name: pack.client_zip_name(VERSION_TOKEN) }
}

pub fn default_server_names(pack: &Pack) -> UploadNames {
    UploadNames { display_name: format!("Server Pack {VERSION_TOKEN}"), file_name: pack.server_zip_name(VERSION_TOKEN) }
}

fn upload_names(title: &str, overrides: &UploadNames, defaults: UploadNames, version: &str) -> Result<UploadNames> {
    let resolve = |name: &str, default: String| Some(name.trim()).filter(|n| !n.is_empty()).map_or(default, String::from).replace(VERSION_TOKEN, version);
    let display_name = resolve(&overrides.display_name, defaults.display_name);
    let mut file_name = resolve(&overrides.file_name, defaults.file_name);
    if file_name.contains(UNSAFE_FILE_NAME_CHARS) {
        bail!("The {title} file name can't contain any of / \\ : * ? \" < > |.");
    }
    if file_name.eq_ignore_ascii_case(".zip") {
        bail!("The {title} file name needs a name before .zip.");
    }
    if !file_name.to_ascii_lowercase().ends_with(".zip") {
        file_name.push_str(".zip");
    }
    Ok(UploadNames { display_name, file_name })
}

pub fn preflight_warnings(pack: &Pack) -> Vec<String> {
    let disabled: Vec<&str> = pack.lock.files.iter().filter_map(|e| e.path.strip_suffix(".disabled")?.split_once('/').map(|(_, name)| name)).collect();
    let mut warnings = match disabled[..] {
        [] => vec![],
        [one] => vec![format!("{one} is disabled, so packraven will leave it out of the export.")],
        _ => vec![format!("{} files are disabled, so packraven will leave them out of the export: {}.", disabled.len(), disabled.join(", "))],
    };
    if pack.version().is_some() && !pack.has_bcc() {
        warnings.push("Better Compatibility Checker isn't in the mods folder, so the pack won't show its version in game.".into());
    }
    warnings
}

pub fn tag_exists(pack: &Pack, version: &str) -> bool {
    git::run(&pack.dir, &["rev-parse", "-q", "--verify", &format!("refs/tags/v{version}")]).is_ok()
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
        problems.push("This packraven build has no CurseForge API key. Build it with PACKRAVEN_CF_API_KEY set.".into());
    }
    if pack.version().is_none() {
        problems.push(if pack.has_bcc() {
            "Better Compatibility Checker has no config yet, so the pack has no version. Create it in Changes.".into()
        } else {
            "Install Better Compatibility Checker in Changes. packraven keeps the pack version in BCC's config.".into()
        });
    } else if let Some(problem) = version_problem(version) {
        problems.push(format!("{problem} Change the version to publish."));
    } else if tag_exists(pack, version) {
        problems.push(format!("You already released {version} (tag v{version} exists). Change the version to publish a new release."));
    }
    match not_on_curseforge {
        [] => {}
        [path] => problems.push(format!("{path} isn't on CurseForge. In Changes, add it to Git or move it to the trash.")),
        paths => problems.push(format!("{} files aren't on CurseForge. In Changes, add them to Git or move them to the trash.", paths.len())),
    }
    problems.extend(export::duplicate_mods_problem(pack));
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
    pub data_dir: PathBuf,
    pub pack_data: PathBuf,
    pub cache_path: PathBuf,
    pub announce: bool,
}

pub async fn publish(ctx: PublishContext, options: PublishOptions, log: Log) -> Result<String> {
    let PublishContext { pack, identity, data_dir, pack_data, cache_path, announce } = ctx;
    let version = options.version.trim().to_string();
    let _guard = modsync::MOD_LIST_GUARD.lock().await;
    let unknown = modsync::refresh_mod_list(&pack.dir).await?;
    let problems = preflight(&pack, &version, &unknown, &identity);
    if !problems.is_empty() {
        bail!("{}", problems.join("\n"));
    }
    let pack = Pack::open(&pack.dir)?;
    let project_id = pack.config.curseforge.project_id;
    let client_names = upload_names("client pack", &options.client, default_client_names(&pack), &version)?;
    let server_names = upload_names("server pack", &options.server, default_server_names(&pack), &version)?;

    pack.set_version(&version)?;
    log.info(format!("Set the pack version to {version}"));
    let out_dir = pack_data.join("builds").join(&version);
    let client_zip = export::client_export(&pack, &version, &out_dir, &log)?;

    let mut cache = MetadataCache::load(&cache_path);
    let sides = sides::classify(&pack, &mut cache).await?;
    let baseline_path = pack_data.join(changelog::BASELINE_FILE);
    let diff = changelog::mod_diff(&pack, &pack.lock, &mut cache, &baseline_path).await?;
    cache.save(&cache_path)?;
    export::server_pack(&pack, &sides, &version, &out_dir, &data_dir, &log).await?;
    let server_zip = export::zip_server_pack(&pack, &version, &out_dir, &log).await?;
    let notes = changelog::render(&changelog::read_notes(&pack), &diff);

    let game_versions = curseforge::game_version_ids(pack.lock.minecraft()?, pack.lock.mod_loader()?.name()).await?;
    let release_type = pack.config.curseforge.release_type.clone();
    log.info(format!("Uploading {} as {}", client_zip.display(), client_names.file_name));
    let file_id = curseforge::upload(
        project_id,
        &client_zip,
        client_names.file_name,
        json!({
            "changelog": notes,
            "changelogType": "markdown",
            "displayName": client_names.display_name,
            "gameVersions": game_versions,
            "releaseType": release_type,
            "isMarkedForManualRelease": pack.config.curseforge.manual_release,
        }),
    )
    .await?;
    log.info(format!("Uploaded the client pack as file {file_id}"));
    log.info(format!("Uploading {} as {}", server_zip.display(), server_names.file_name));
    let server_file_id = curseforge::upload(
        project_id,
        &server_zip,
        server_names.file_name,
        json!({
            "changelog": notes,
            "changelogType": "markdown",
            "displayName": server_names.display_name,
            "parentFileID": file_id,
            "releaseType": release_type,
        }),
    )
    .await
    .context("packraven uploaded the client pack, but the server pack upload failed; upload the server pack as an additional file on CurseForge")?;
    log.info(format!("Uploaded the server pack as file {server_file_id}"));

    let releases_path = pack_data.join("releases.json");
    let mut releases = load_releases(&releases_path);
    let curseforge = Some(CurseforgeRelease { project_id, file_id, server_file_id });
    releases.push(Release { version: version.clone(), curseforge, modrinth: None, published_at: chrono::Utc::now().to_rfc3339(), announce, announced_at: None });
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

#[cfg(test)]
mod tests {
    use super::{load_releases, upload_names, UploadNames};

    fn names(display_name: &str, file_name: &str) -> UploadNames {
        UploadNames { display_name: display_name.into(), file_name: file_name.into() }
    }

    fn resolve(overrides: UploadNames) -> anyhow::Result<UploadNames> {
        upload_names("client pack", &overrides, names("Pack {version}", "Pack-{version}.zip"), "1.0")
    }

    #[test]
    fn upload_names_fall_back_to_the_defaults_with_the_version_filled_in() {
        assert_eq!(resolve(names(" ", "")).unwrap(), names("Pack 1.0", "Pack-1.0.zip"));
    }

    #[test]
    fn upload_names_fill_the_version_into_overrides() {
        assert_eq!(resolve(names("Winter {version}", "winter-{version}")).unwrap(), names("Winter 1.0", "winter-1.0.zip"));
    }

    #[test]
    fn upload_names_trim_overrides_and_add_zip_to_file_names() {
        assert_eq!(resolve(names(" My Pack ", " my-pack ")).unwrap(), names("My Pack", "my-pack.zip"));
        assert_eq!(resolve(names("", "pack.ZIP")).unwrap(), names("Pack 1.0", "pack.ZIP"));
    }

    #[test]
    fn reads_releases_from_before_modrinth() {
        let path = std::env::temp_dir().join(format!("packraven-releases-{}.json", std::process::id()));
        std::fs::write(&path, r#"[{"version": "1.0", "projectId": 5, "fileId": 6, "serverFileId": 7, "publishedAt": "2026-10-01T00:00:00Z"}]"#).unwrap();
        let releases = load_releases(&path);
        std::fs::remove_file(&path).unwrap();
        let curseforge = releases[0].curseforge.as_ref().unwrap();
        assert_eq!((curseforge.project_id, curseforge.file_id, curseforge.server_file_id), (5, 6, 7));
        assert!(releases[0].modrinth.is_none() && !releases[0].announce);
    }

    #[test]
    fn upload_names_reject_unsafe_file_names() {
        assert!(resolve(names("", "dir/pack")).is_err());
        assert!(resolve(names("", "pack?")).is_err());
        assert!(resolve(names("", ".zip")).is_err());
    }
}
