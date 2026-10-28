use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;
use serde_json::json;
use tokio::process::Command;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::git;
use crate::http;
use crate::jobs::Log;
use crate::mirror::{mirror, FilePlan, Planned};
use crate::pack::{LockEntry, ModKey, ModLoader, Pack, SERVER_ICON};
use crate::sides::{ModSide, Side};

const NEOFORGE_MAVEN: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";
const FORGE_MAVEN: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";
const FABRIC_META: &str = "https://meta.fabricmc.net/v2/versions";
const FABRIC_LAUNCHER: &str = "fabric-server-launch.jar";
const RUN_BAT: &str = include_str!("scripts/run.bat");
const RUN_SH: &str = include_str!("scripts/run.sh");
const INSTALL_BAT: &str = include_str!("scripts/install.bat");
const INSTALL_SH: &str = include_str!("scripts/install.sh");

fn globs(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern).with_context(|| format!("invalid pattern {pattern}"))?);
    }
    Ok(builder.build()?)
}

pub fn override_files(pack: &Pack, extra_exclude: &[String]) -> Result<Vec<String>> {
    let excluded = globs(&[pack.config.export.exclude.clone(), extra_exclude.to_vec()].concat())?;
    Ok(git::tracked_files(&pack.dir)?.into_iter().filter(|path| !excluded.is_match(path) && pack.dir.join(path).is_file()).collect())
}

// We check these on every overview refresh, so we ask git for tracked jars instead of checking
// every override file.
pub fn unallowed_override_jars(pack: &Pack) -> Result<Vec<String>> {
    let excluded = globs(&pack.config.export.exclude)?;
    let allowed = globs(&pack.config.export.allowed_override_jars)?;
    let jars = git::run(&pack.dir, &["ls-files", "-z", "--", "*.jar"])?;
    Ok(jars
        .split('\0')
        .filter(|path| path.ends_with(".jar") && !excluded.is_match(path) && !allowed.is_match(path) && pack.dir.join(path).is_file())
        .map(String::from)
        .collect())
}

// The CurseForge app can leave the old jar behind when it can't delete it during an update. Loaders
// try to load only the newest of two versions, but sometimes pick the wrong one, and the platforms
// reject a manifest that lists one project twice. Packs can hold several variants of one resource
// pack, so we only check mods.
pub fn duplicate_mods(pack: &Pack) -> Vec<Vec<String>> {
    let mut by_project: BTreeMap<ModKey, Vec<String>> = BTreeMap::new();
    for entry in pack.lock.files.iter().filter(|e| e.path.starts_with("mods/") && !e.path.ends_with(".disabled")) {
        if let Some(key) = entry.key() {
            by_project.entry(key).or_default().push(entry.path.trim_start_matches("mods/").to_string());
        }
    }
    by_project.into_values().filter(|paths| paths.len() > 1).collect()
}

pub fn duplicate_mods_problem(pack: &Pack) -> Option<String> {
    let groups = duplicate_mods(pack);
    let listed: Vec<String> = groups.iter().map(|files| files.join(" and ")).collect();
    match groups.len() {
        0 => None,
        1 => Some(format!("The mods folder has two versions of one mod: {}. In Mods, remove the one you don't want.", listed[0])),
        _ => Some(format!("The mods folder has two versions of {} mods: {}. In Mods, remove the ones you don't want.", groups.len(), listed.join("; "))),
    }
}

pub fn local_dev_jars(pack: &Pack) -> Vec<String> {
    std::fs::read_dir(pack.dir.join("mods"))
        .map(|dir| {
            dir.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with("-dev.jar"))
                .collect()
        })
        .unwrap_or_default()
}

fn options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated).large_file(true)
}

fn add_file(zip: &mut ZipWriter<File>, name: &str, source: &Path, executable: bool) -> Result<()> {
    let already_compressed = name.ends_with(".jar") || name.ends_with(".zip");
    let options = if already_compressed { options().compression_method(CompressionMethod::Stored) } else { options() };
    let options = if executable { options.unix_permissions(0o755) } else { options };
    zip.start_file(name, options)?;
    std::io::copy(&mut File::open(source).with_context(|| format!("reading {}", source.display()))?, zip)?;
    Ok(())
}

pub fn client_export(pack: &Pack, version: &str, out_dir: &Path, log: &Log) -> Result<PathBuf> {
    std::fs::create_dir_all(out_dir)?;
    let path = out_dir.join(pack.client_zip_name(version));
    let files: Vec<_> = pack
        .lock
        .files
        .iter()
        .filter(|e| !e.path.ends_with(".disabled"))
        .filter_map(|e| Some((e, e.curseforge.as_ref()?)))
        .map(|(e, f)| json!({ "projectID": f.project_id, "fileID": f.file_id, "required": true, "isLocked": pack.config.updates.locked.iter().any(|key| e.has_key(key)) }))
        .collect();
    let manifest = json!({
        "minecraft": {
            "version": pack.lock.minecraft()?,
            "modLoaders": [{ "id": pack.lock.mod_loader()?.manifest_id(), "primary": true }],
            "recommendedRam": pack.config.export.recommended_ram_mb,
        },
        "manifestType": "minecraftModpack",
        "manifestVersion": 1,
        "name": pack.name(),
        "version": version,
        "author": pack.config.author,
        "files": files,
        "overrides": "overrides",
    });
    if let Some(problem) = duplicate_mods_problem(pack) {
        bail!(problem);
    }
    let jars = unallowed_override_jars(pack)?;
    if !jars.is_empty() {
        bail!(
            "CurseForge rejects packs with jars in overrides: {}. Remove them, or list them under export.allowed_override_jars in packraven.toml.",
            jars.join(", ")
        );
    }
    let overrides = override_files(pack, &[SERVER_ICON.to_string()])?;
    let mut zip = ZipWriter::new(File::create(&path)?);
    zip.start_file("manifest.json", options())?;
    zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
    for file in &overrides {
        let name = format!("overrides/{file}");
        match pack.config.export.properties.get(file) {
            Some(forced) => {
                zip.start_file(name, options())?;
                zip.write_all(force_properties(&std::fs::read_to_string(pack.dir.join(file))?, forced).as_bytes())?;
            }
            None => add_file(&mut zip, &name, &pack.dir.join(file), false)?,
        }
    }
    zip.finish()?;
    log.info(format!("Client export: {} CurseForge files, {} override files", files.len(), overrides.len()));
    Ok(path)
}

fn force_properties(text: &str, forced: &BTreeMap<String, String>) -> String {
    let mut remaining: Vec<&String> = forced.keys().collect();
    let mut out: Vec<String> = text
        .lines()
        .map(|line| {
            let key = line.split('=').next().unwrap_or_default().trim();
            match forced.get(key) {
                Some(value) if !line.trim_start().starts_with('#') => {
                    remaining.retain(|k| k.as_str() != key);
                    format!("{key}={value}")
                }
                _ => line.to_string(),
            }
        })
        .collect();
    out.extend(remaining.into_iter().map(|key| format!("{key}={}", forced[key])));
    out.join("\n") + "\n"
}

pub async fn ensure_cached(pack: &Pack, entry: &LockEntry) -> Result<PathBuf> {
    let path = pack.cache_dir()?.join(&entry.sha1);
    if !path.exists() {
        let url = entry.download_url().with_context(|| format!("packraven has no download for {}", entry.path))?;
        http::download(url, &path, Some(&entry.sha1)).await?;
    }
    Ok(path)
}

#[derive(Deserialize)]
struct FabricInstaller {
    url: String,
    version: String,
    stable: bool,
}

async fn stable_fabric_installer() -> Result<FabricInstaller> {
    let installers: Vec<FabricInstaller> = http::CLIENT.get(format!("{FABRIC_META}/installer")).send().await?.error_for_status()?.json().await?;
    installers.into_iter().find(|i| i.stable).context("Fabric lists no stable installer")
}

fn forge_installer_url(minecraft: &str, loader: &ModLoader) -> Option<String> {
    match loader {
        ModLoader::NeoForge(version) => Some(format!("{NEOFORGE_MAVEN}/{version}/neoforge-{version}-installer.jar")),
        ModLoader::Forge(version) => Some(format!("{FORGE_MAVEN}/{minecraft}-{version}/forge-{minecraft}-{version}-installer.jar")),
        ModLoader::Fabric(_) => None,
    }
}

fn url_file_name(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or(url)
}

async fn download_once(url: &str, path: &Path, what: &str, log: &Log) -> Result<PathBuf> {
    if !path.exists() {
        log.info(format!("Downloading {what}"));
        http::download(url, path, None).await?;
    }
    Ok(path.to_path_buf())
}

async fn fabric_server_launcher(pack: &Pack, folder: &Path, log: &Log) -> Result<PathBuf> {
    let minecraft = pack.lock.minecraft()?;
    let ModLoader::Fabric(version) = pack.lock.mod_loader()? else { bail!("only Fabric packs have a Fabric server launcher") };
    let installer = stable_fabric_installer().await?.version;
    let url = format!("{FABRIC_META}/loader/{minecraft}/{version}/{installer}/server/jar");
    let cached = folder.join(format!("fabric-server-mc.{minecraft}-loader.{version}-launcher.{installer}.jar"));
    download_once(&url, &cached, "Fabric's server launcher", log).await
}

async fn loader_server_base(pack: &Pack, java: &Path, data_dir: &Path, log: &Log) -> Result<PathBuf> {
    let minecraft = pack.lock.minecraft()?;
    let loader = pack.lock.mod_loader()?;
    let folder = data_dir.join(loader.name().to_lowercase());
    let name = match loader {
        ModLoader::NeoForge(version) => format!("server-{version}"),
        ModLoader::Forge(version) | ModLoader::Fabric(version) => format!("server-{minecraft}-{version}"),
    };
    let base = folder.join(&name);
    if base.join("libraries").exists() {
        return Ok(base);
    }
    let (installer_url, args) = match (forge_installer_url(minecraft, &loader), &loader) {
        (Some(url), _) => (url, vec!["--installServer"]),
        (None, ModLoader::Fabric(version)) => (stable_fabric_installer().await?.url, vec!["server", "-mcversion", minecraft, "-loader", version, "-downloadMinecraft", "-dir"]),
        (None, _) => bail!("packraven doesn't know where to download the {} installer", loader.name()),
    };
    let installer = download_once(&installer_url, &folder.join(url_file_name(&installer_url)), &format!("the {} installer", loader.name()), log).await?;
    let staging = folder.join(format!("{name}-staging"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    log.info(format!("Installing the {} server for Minecraft {minecraft}", loader.name()));
    let mut command = Command::new(java);
    command.current_dir(&staging).arg("-jar").arg(&installer).args(args).arg(&staging);
    crate::process::run_logged(command, log).await.with_context(|| format!("{} server install failed", loader.name()))?;
    if let ModLoader::Fabric(_) = loader {
        unpack_vanilla_bundler(&staging, java, log).await?;
    }
    for entry in std::fs::read_dir(&staging)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().ends_with(".log") {
            std::fs::remove_file(entry.path())?;
        }
    }
    std::fs::rename(&staging, &base)?;
    Ok(base)
}

// Since 1.18, vanilla's server.jar bundles its libraries and unpacks them on every start that
// finds them missing. The NeoForge and Forge installers unpack them up front, but Fabric's
// doesn't, so we do it here. Otherwise each test run's sync deletes them again.
async fn unpack_vanilla_bundler(dir: &Path, java: &Path, log: &Log) -> Result<()> {
    let bundled = File::open(dir.join("server.jar")).ok().and_then(|jar| zip::ZipArchive::new(jar).ok()).is_some_and(|mut jar| jar.by_name("META-INF/versions.list").is_ok());
    if !bundled {
        return Ok(());
    }
    log.info("Unpacking the vanilla server's libraries");
    let mut command = Command::new(java);
    // Vanilla's data generator prints its usage and exits once the bundler has unpacked everything.
    command.current_dir(dir).arg("-DbundlerMainClass=net.minecraft.data.Main").arg("-jar").arg("server.jar").arg("--help");
    crate::process::run_logged(command, log).await.context("unpacking the vanilla server failed")?;
    let _ = std::fs::remove_dir_all(dir.join("logs"));
    Ok(())
}

pub async fn test_server_plan(pack: &Pack, sides: &[ModSide], version: &str, java: &Path, data_dir: &Path, log: &Log) -> Result<FilePlan> {
    let base = loader_server_base(pack, java, data_dir, log).await?;
    let mut plan = FilePlan::new();
    for entry in WalkDir::new(&base) {
        let entry = entry?;
        if entry.file_type().is_file() {
            let relative = entry.path().strip_prefix(&base)?.to_string_lossy().replace('\\', "/");
            plan.insert(relative, Planned::Copy(entry.into_path()));
        }
    }
    plan.extend(pack_server_files(pack, sides, version, log).await?);
    Ok(plan)
}

// The Minecraft EULA doesn't let us redistribute vanilla's server, so release server packs carry the
// loader's installer (or Fabric's server launcher) instead, and that downloads it from Mojang on first start.
async fn release_server_plan(pack: &Pack, sides: &[ModSide], version: &str, data_dir: &Path, log: &Log) -> Result<FilePlan> {
    let mut plan = pack_server_files(pack, sides, version, log).await?;
    let loader = pack.lock.mod_loader()?;
    let folder = data_dir.join(loader.name().to_lowercase());
    match forge_installer_url(pack.lock.minecraft()?, &loader) {
        Some(url) => {
            let name = url_file_name(&url);
            let installer = download_once(&url, &folder.join(name), &format!("the {} installer", loader.name()), log).await?;
            plan.insert(name.to_string(), Planned::Copy(installer));
        }
        None => {
            plan.insert(FABRIC_LAUNCHER.into(), Planned::Copy(fabric_server_launcher(pack, &folder, log).await?));
        }
    }
    Ok(plan)
}

async fn pack_server_files(pack: &Pack, sides: &[ModSide], version: &str, log: &Log) -> Result<FilePlan> {
    let mut plan = FilePlan::new();
    let client_only: HashSet<&str> = sides.iter().filter(|m| m.side == Side::Client).map(|m| m.path.as_str()).collect();
    let server_files: Vec<&LockEntry> = pack
        .lock
        .files
        .iter()
        .filter(|e| !e.path.ends_with(".disabled") && !client_only.contains(e.path.as_str()))
        .filter(|e| e.path.starts_with("mods/") || e.path.starts_with("datapacks/"))
        .collect();
    log.info(format!("Including {} server files ({} client-only skipped)", server_files.len(), client_only.len()));
    for entry in &server_files {
        plan.insert(entry.path.clone(), Planned::Copy(ensure_cached(pack, entry).await?));
    }

    let overrides = override_files(pack, &pack.config.server.exclude)?;
    log.info(format!("Including {} config and override files", overrides.len()));
    for file in overrides {
        let source = pack.dir.join(&file);
        plan.insert(file, Planned::Copy(source));
    }
    // We ship the icon before it's committed, so a test run shows the one you just picked.
    let icon = pack.dir.join(SERVER_ICON);
    if icon.is_file() {
        plan.insert(SERVER_ICON.into(), Planned::Copy(icon));
    }

    let mut properties = pack.config.server.properties.clone();
    let motd = fill_motd(properties.get("motd").map_or(DEFAULT_MOTD, String::as_str), &pack.name(), version, pack.lock.minecraft.as_deref().unwrap_or_default());
    properties.insert("motd".into(), motd);
    plan.insert("server.properties".into(), Planned::Write(properties.iter().map(|(k, v)| format!("{k}={}\n", escape_property(v))).collect()));
    plan.insert("user_jvm_args.txt".into(), Planned::Write(user_jvm_args(pack)));
    // We replace the NeoForge and Forge installers' run scripts, which call whatever java is on
    // PATH, and Fabric's installer writes none.
    let graalvm = server_graalvm(pack)?;
    let args = &pack.config.server.args;
    let run_bat = start_script(RUN_BAT, &install_step(pack, true)?, &loader_launch(pack, true)?, graalvm.as_deref(), &script_args(args, true)).replace('\n', "\r\n");
    plan.insert("run.bat".into(), Planned::Write(run_bat));
    plan.insert("run.sh".into(), Planned::Write(start_script(RUN_SH, &install_step(pack, false)?, &loader_launch(pack, false)?, graalvm.as_deref(), &script_args(args, false))));
    Ok(plan)
}

// Fabric's server launcher installs everything itself when it starts, so only NeoForge and Forge need this.
fn install_step(pack: &Pack, windows: bool) -> Result<String> {
    let loader = pack.lock.mod_loader()?;
    let Some(url) = forge_installer_url(pack.lock.minecraft()?, &loader) else { return Ok(String::new()) };
    let launch = loader_launch(pack, windows)?;
    let launched = launched_file(&launch);
    let (template, launched) = if windows { (INSTALL_BAT, launched.replace('/', "\\")) } else { (INSTALL_SH, launched.to_string()) };
    Ok(template.replace("{{LOADER}}", loader.name()).replace("{{INSTALLER}}", url_file_name(&url)).replace("{{LAUNCHED}}", &launched))
}

const DEFAULT_MOTD: &str = "{name} {version}";

fn fill_motd(template: &str, name: &str, version: &str, minecraft: &str) -> String {
    template.replace("{name}", name).replace("{version}", version).replace("{minecraft}", minecraft)
}

// Older Minecraft versions read server.properties as Latin-1, so a MOTD's section signs and other
// non-ASCII text only survive as Java's \uXXXX escapes.
fn escape_property(value: &str) -> String {
    value.encode_utf16().map(|unit| if unit < 0x80 { char::from(unit as u8).to_string() } else { format!("\\u{unit:04X}") }).collect()
}

fn user_jvm_args(pack: &Pack) -> String {
    let mut text = format!("-Xmx{}\n", pack.config.server.memory);
    for flag in pack.config.server.jvm_args.iter().map(|flag| flag.trim()).filter(|flag| !flag.is_empty()) {
        text.push_str(flag);
        text.push('\n');
    }
    text
}

fn script_args(args: &[String], windows: bool) -> String {
    args.iter()
        .map(|arg| arg.trim())
        .filter(|arg| !arg.is_empty())
        .map(|arg| {
            let plain = arg.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_=.,:/@+".contains(&b));
            match (plain, windows) {
                (true, _) => format!(" {arg}"),
                // cmd expands %VAR% even inside quotes, so we double the percent signs.
                (false, true) => format!(" \"{}\"", arg.replace('%', "%%").replace('"', "")),
                (false, false) => format!(" '{}'", arg.replace('\'', "'\\''")),
            }
        })
        .collect()
}

pub fn loader_launch(pack: &Pack, windows: bool) -> Result<String> {
    let args_file = if windows { "win_args.txt" } else { "unix_args.txt" };
    Ok(match pack.lock.mod_loader()? {
        ModLoader::NeoForge(version) => format!("@libraries/net/neoforged/neoforge/{version}/{args_file}"),
        ModLoader::Forge(version) => format!("@libraries/net/minecraftforge/forge/{}-{version}/{args_file}", pack.lock.minecraft()?),
        ModLoader::Fabric(_) => format!("-jar {FABRIC_LAUNCHER}"),
    })
}

fn launched_file(launch: &str) -> &str {
    launch.trim_start_matches('@').trim_start_matches("-jar ")
}

pub fn server_args(pack: &Pack, dir: &Path, extra_jvm: &[String], extra: &[String]) -> Result<Vec<String>> {
    let launch = loader_launch(pack, cfg!(windows))?;
    let launched_file = launched_file(&launch);
    if !dir.join(launched_file).exists() {
        bail!("the {} installer didn't create {launched_file}, so packraven can't start this server", pack.lock.mod_loader()?.name());
    }
    let args = pack.config.server.args.iter().map(|arg| arg.trim()).filter(|arg| !arg.is_empty());
    Ok(["@user_jvm_args.txt"]
        .into_iter()
        .chain(extra_jvm.iter().map(String::as_str))
        .chain(launch.split(' '))
        .chain(["nogui"])
        .chain(args)
        .chain(extra.iter().map(String::as_str))
        .map(String::from)
        .collect())
}

fn server_graalvm(pack: &Pack) -> Result<Option<String>> {
    let configured = pack.config.server.graalvm.trim();
    if configured.is_empty() {
        return Ok(crate::java::graalvm_for(crate::launch::java_for_minecraft(pack.lock.minecraft()?).1).map(|major| major.to_string()));
    }
    let parts: Vec<&str> = configured.split('.').collect();
    let valid = parts.len() <= 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) && parts[0].parse::<u32>().is_ok_and(|major| major >= 17);
    if !valid {
        bail!("server.graalvm in packraven.toml must be a GraalVM version like 21 or 21.0.7, found {configured}");
    }
    Ok(Some(configured.to_string()))
}

fn graalvm_url_prefix(version: &str) -> String {
    match version.split_once('.') {
        Some((major, _)) => format!("https://download.oracle.com/graalvm/{major}/archive/graalvm-jdk-{version}_"),
        None => format!("https://download.oracle.com/graalvm/{version}/latest/graalvm-jdk-{version}_"),
    }
}

fn start_script(template: &str, install: &str, launch: &str, graalvm: Option<&str>, args: &str) -> String {
    template
        .replace("{{INSTALL}}", install)
        .replace("\r\n", "\n")
        .replace("{{GRAALVM}}", graalvm.unwrap_or_default())
        .replace("{{URL_PREFIX}}", &graalvm.map(graalvm_url_prefix).unwrap_or_default())
        .replace("{{LAUNCH}}", launch)
        .replace("{{ARGS}}", args)
}

pub async fn server_pack(pack: &Pack, sides: &[ModSide], version: &str, out_dir: &Path, data_dir: &Path, log: &Log) -> Result<PathBuf> {
    let dir = out_dir.join("server");
    let plan = release_server_plan(pack, sides, version, data_dir, log).await?;
    log.info(mirror(&dir, &plan, &[], false)?.to_string());
    Ok(dir)
}

pub async fn zip_server_pack(pack: &Pack, version: &str, out_dir: &Path, log: &Log) -> Result<PathBuf> {
    let dir = out_dir.join("server");
    let zip_path = out_dir.join(pack.server_zip_name(version));
    let files = WalkDir::new(&dir).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()).count();
    log.info(format!("Zipping the server pack ({files} files). This can take a few minutes."));
    let (source, dest) = (dir, zip_path.clone());
    tokio::task::spawn_blocking(move || zip_dir(&source, &dest)).await??;
    log.info(format!("Server pack: {}", zip_path.display()));
    Ok(zip_path)
}

fn zip_dir(source: &Path, dest: &Path) -> Result<()> {
    let mut zip = ZipWriter::new(File::create(dest)?);
    for entry in WalkDir::new(source).sort_by_file_name() {
        let entry = entry?;
        if entry.file_type().is_file() {
            let name = entry.path().strip_prefix(source)?.to_string_lossy().replace('\\', "/");
            add_file(&mut zip, &name, entry.path(), name.ends_with(".sh"))?;
        }
    }
    zip.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn force_properties_replaces_and_appends() {
        let forced: BTreeMap<String, String> = [("enableShaders", "false"), ("added", "1")].into_iter().map(|(k, v)| (k.into(), v.into())).collect();
        let text = "#enableShaders=true\nenableShaders=true\nshaderPack=x\n";
        assert_eq!(force_properties(text, &forced), "#enableShaders=true\nenableShaders=false\nshaderPack=x\nadded=1\n");
    }

    #[test]
    fn fills_motd_placeholders() {
        assert_eq!(fill_motd(DEFAULT_MOTD, "Ravens", "1.2.0", "1.21.1"), "Ravens 1.2.0");
        assert_eq!(fill_motd("{name} for {minecraft}\\nv{version}", "Ravens", "1.2.0", "1.21.1"), "Ravens for 1.21.1\\nv1.2.0");
    }

    #[test]
    fn escapes_non_ascii_properties() {
        assert_eq!(escape_property("\u{a7}aRavens \u{2694}\\n1.0"), "\\u00A7aRavens \\u2694\\n1.0");
    }

    #[test]
    fn quotes_script_args_that_need_it() {
        let args: Vec<String> = ["--world", "My World", "", "100%"].map(String::from).to_vec();
        assert_eq!(script_args(&args, true), " --world \"My World\" \"100%%\"");
        assert_eq!(script_args(&args, false), " --world 'My World' '100%'");
        assert_eq!(script_args(&["it's".into()], false), " 'it'\\''s'");
    }

    fn loader_pack(minecraft: &str, loader: &str) -> Pack {
        Pack { dir: PathBuf::new(), config: Default::default(), lock: crate::pack::Lockfile { minecraft: Some(minecraft.into()), loader: crate::pack::Loader::from_curseforge(loader), files: Vec::new() } }
    }

    #[test]
    fn neoforge_scripts_install_on_first_start() {
        let pack = loader_pack("1.21.1", "neoforge-21.1.259");
        let bat = install_step(&pack, true).unwrap();
        assert!(bat.contains(r#"if exist "libraries\net\neoforged\neoforge\21.1.259\win_args.txt" goto run"#), "{bat}");
        assert!(bat.contains(r#""%JAVA%" -jar "neoforge-21.1.259-installer.jar" --installServer .install"#), "{bat}");
        let sh = install_step(&pack, false).unwrap();
        assert!(sh.contains(r#"if [ ! -f "libraries/net/neoforged/neoforge/21.1.259/unix_args.txt" ]"#), "{sh}");
        let script = start_script(RUN_SH, &sh, &loader_launch(&pack, false).unwrap(), None, "");
        assert!(!script.contains("{{"), "{script}");
    }

    #[test]
    fn fabric_scripts_have_no_install_step() {
        assert_eq!(install_step(&loader_pack("1.21.1", "fabric-0.16.14-1.21.1"), true).unwrap(), "");
    }

    #[test]
    fn finds_two_enabled_versions_of_one_mod() {
        let entry = |path: &str, project_id| LockEntry::curseforge(path.into(), String::new(), 0, crate::pack::CurseforgeFile { project_id, file_id: 0, url: String::new() });
        let files = vec![
            entry("mods/sodium-0.6.14.jar", 1),
            entry("mods/sodium-0.6.15.jar", 1),
            entry("mods/iris-1.8.jar", 2),
            entry("mods/iris-1.7.jar.disabled", 2),
            entry("resourcepacks/x-dark.zip", 3),
            entry("resourcepacks/x-light.zip", 3),
        ];
        let pack = Pack { dir: PathBuf::new(), config: Default::default(), lock: crate::pack::Lockfile { minecraft: None, loader: None, files } };
        assert_eq!(duplicate_mods(&pack), [["sodium-0.6.14.jar", "sodium-0.6.15.jar"]]);
    }
}
