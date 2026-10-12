use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde_json::json;
use tokio::process::Command;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::git;
use crate::http;
use crate::jobs::Log;
use crate::pack::{LockEntry, Pack};
use crate::sides::{ModSide, Side};

const NEOFORGE_MAVEN: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";

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

pub fn unallowed_override_jars(pack: &Pack) -> Result<Vec<String>> {
    let allowed = globs(&pack.config.export.allowed_override_jars)?;
    Ok(override_files(pack, &[])?.into_iter().filter(|p| p.ends_with(".jar") && !allowed.is_match(p)).collect())
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
    let path = out_dir.join(format!("{}.zip", pack.file_stem(version)));
    let files: Vec<_> = pack
        .lock
        .files
        .iter()
        .filter(|e| !e.path.ends_with(".disabled"))
        .map(|e| json!({ "projectID": e.project_id, "fileID": e.file_id, "required": true, "isLocked": false }))
        .collect();
    let manifest = json!({
        "minecraft": {
            "version": pack.lock.minecraft()?,
            "modLoaders": [{ "id": pack.lock.loader()?, "primary": true }],
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
    let jars = unallowed_override_jars(pack)?;
    if !jars.is_empty() {
        bail!(
            "CurseForge rejects packs with jars in overrides: {}. Remove them, or list them under export.allowed_override_jars in packraven.toml.",
            jars.join(", ")
        );
    }
    let overrides = override_files(pack, &[])?;
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
        http::download(&entry.url, &path, Some(&entry.sha1)).await?;
    }
    Ok(path)
}

async fn neoforge_server_base(version: &str, java: &Path, data_dir: &Path, log: &Log) -> Result<PathBuf> {
    let base = data_dir.join("neoforge").join(format!("server-{version}"));
    if base.join("libraries").exists() {
        return Ok(base);
    }
    let installer = data_dir.join("neoforge").join(format!("neoforge-{version}-installer.jar"));
    if !installer.exists() {
        log.info(format!("Downloading NeoForge {version} installer"));
        http::download(&format!("{NEOFORGE_MAVEN}/{version}/neoforge-{version}-installer.jar"), &installer, None).await?;
    }
    let staging = base.with_file_name(format!("server-{version}-staging"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    log.info(format!("Installing NeoForge {version} server"));
    let mut command = Command::new(java);
    command.current_dir(&staging).arg("-jar").arg(&installer).arg("--installServer").arg(&staging);
    crate::process::run_logged(command, log).await.context("NeoForge server install failed")?;
    for entry in std::fs::read_dir(&staging)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().ends_with(".log") {
            std::fs::remove_file(entry.path())?;
        }
    }
    std::fs::rename(&staging, &base)?;
    Ok(base)
}

pub fn copy_dir(source: &Path, dest: &Path) -> Result<()> {
    for entry in WalkDir::new(source) {
        let entry = entry?;
        let target = dest.join(entry.path().strip_prefix(source)?);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

pub async fn server_pack(pack: &Pack, sides: &[ModSide], version: &str, out_dir: &Path, java: &Path, data_dir: &Path, log: &Log) -> Result<PathBuf> {
    let dir = out_dir.join("server");
    let _ = std::fs::remove_dir_all(&dir);
    let base = neoforge_server_base(pack.lock.neoforge_version()?, java, data_dir, log).await?;
    copy_dir(&base, &dir)?;

    let client_only: HashSet<&str> = sides.iter().filter(|m| m.side == Side::Client).map(|m| m.path.as_str()).collect();
    let server_files: Vec<&LockEntry> = pack
        .lock
        .files
        .iter()
        .filter(|e| !e.path.ends_with(".disabled") && !client_only.contains(e.path.as_str()))
        .filter(|e| e.path.starts_with("mods/") || e.path.starts_with("datapacks/"))
        .collect();
    log.info(format!("Copying {} server files ({} client-only skipped)", server_files.len(), client_only.len()));
    for entry in &server_files {
        let cached = ensure_cached(pack, entry).await?;
        let target = dir.join(&entry.path);
        std::fs::create_dir_all(target.parent().context("lock path has no parent")?)?;
        std::fs::copy(cached, target)?;
    }

    let overrides = override_files(pack, &pack.config.server.exclude)?;
    for file in &overrides {
        let target = dir.join(file);
        std::fs::create_dir_all(target.parent().context("override path has no parent")?)?;
        std::fs::copy(pack.dir.join(file), target)?;
    }
    log.info(format!("Copied {} config and override files", overrides.len()));

    let mut properties = pack.config.server.properties.clone();
    properties.entry("motd".into()).or_insert_with(|| format!("{} {version}", pack.name()));
    let text: String = properties.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
    std::fs::write(dir.join("server.properties"), text)?;
    let jvm_args = dir.join("user_jvm_args.txt");
    let mut existing = std::fs::read_to_string(&jvm_args).unwrap_or_default();
    existing.push_str(&format!("\n-Xmx{}\n", pack.config.server.memory));
    std::fs::write(&jvm_args, existing)?;

    Ok(dir)
}

pub async fn zip_server_pack(pack: &Pack, version: &str, out_dir: &Path, log: &Log) -> Result<PathBuf> {
    let dir = out_dir.join("server");
    let zip_path = out_dir.join(format!("{}-server.zip", pack.file_stem(version)));
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
}
