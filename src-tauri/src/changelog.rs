use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::curseforge;
use crate::git;
use crate::pack::{LockEntry, Lockfile, Pack, CHANGELOG, LOCKFILE};
use crate::sides::MetadataCache;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModChange {
    pub name: String,
    pub url: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModDiff {
    pub since: Option<String>,
    pub baseline: bool,
    pub added: Vec<ModChange>,
    pub updated: Vec<ModChange>,
    pub removed: Vec<ModChange>,
}

pub fn read_notes(pack: &Pack) -> String {
    std::fs::read_to_string(pack.dir.join(CHANGELOG)).unwrap_or_default()
}

pub fn write_notes(pack: &Pack, text: &str) -> Result<()> {
    std::fs::write(pack.dir.join(CHANGELOG), text)?;
    Ok(())
}

pub const BASELINE_FILE: &str = "compare-baseline.json";

#[derive(Serialize, Deserialize)]
pub struct Baseline {
    label: String,
    lock: Lockfile,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseForgeManifest {
    name: Option<String>,
    version: Option<String>,
    files: Vec<ManifestFile>,
}

#[derive(Deserialize)]
struct ManifestFile {
    #[serde(rename = "projectID")]
    project_id: u64,
    #[serde(rename = "fileID")]
    file_id: u64,
}

pub fn baseline_from_zip(zip: &Path) -> Result<Baseline> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(zip)?).context("not a zip file")?;
    let mut text = String::new();
    archive.by_name("manifest.json").context("this zip has no manifest.json, so it isn't a CurseForge modpack export")?.read_to_string(&mut text)?;
    let manifest: CurseForgeManifest = serde_json::from_str(&text).context("couldn't read manifest.json")?;
    let label = match (manifest.name, manifest.version) {
        (Some(name), Some(version)) => format!("{name} {version}"),
        _ => zip.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
    };
    let files = manifest
        .files
        .into_iter()
        .map(|f| LockEntry { path: String::new(), project_id: f.project_id, file_id: f.file_id, sha1: String::new(), size: 0, url: String::new() })
        .collect();
    Ok(Baseline { label, lock: Lockfile { files, ..Default::default() } })
}

pub fn save_baseline(path: &Path, baseline: &Baseline) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string(baseline)?)?;
    Ok(())
}

pub fn clear_baseline(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn load_baseline(path: &Path) -> Option<Baseline> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn by_project(lock: &Lockfile) -> BTreeMap<u64, &LockEntry> {
    lock.files.iter().map(|e| (e.project_id, e)).collect()
}

pub async fn mod_diff(pack: &Pack, cache: &mut MetadataCache, baseline_path: &Path) -> Result<ModDiff> {
    let baseline = load_baseline(baseline_path);
    let (since, previous) = match (baseline, git::last_release_tag(&pack.dir)) {
        (Some(baseline), _) => (baseline.label, baseline.lock),
        (None, Some(tag)) => {
            let lock = git::show(&pack.dir, &tag, LOCKFILE).map(|t| Lockfile::parse(&t)).transpose()?.unwrap_or_default();
            (tag, lock)
        }
        (None, None) => return Ok(ModDiff::default()),
    };
    let is_baseline = baseline_path.exists();
    let old = by_project(&previous);
    let new = by_project(&pack.lock);
    let ids: Vec<u64> = old.keys().chain(new.keys()).copied().collect();
    cache.ensure_names(&ids).await?;
    let changed_files: Vec<u64> = new
        .iter()
        .filter_map(|(id, e)| old.get(id).filter(|o| o.file_id != e.file_id).map(|o| [o.file_id, e.file_id]))
        .flatten()
        .collect();
    let file_names: HashMap<u64, String> = if changed_files.is_empty() {
        HashMap::new()
    } else {
        curseforge::files(&changed_files).await?.into_iter().map(|(id, f)| (id, f.display_name)).collect()
    };
    let display = |entry: &LockEntry| {
        file_names.get(&entry.file_id).cloned().unwrap_or_else(|| entry.path.rsplit('/').next().unwrap_or_default().to_string())
    };
    let change = |id: u64, from: Option<String>, to: Option<String>| {
        let name = cache.name(id);
        ModChange { name: name.name, url: name.url, from, to }
    };
    let mut diff = ModDiff { since: Some(since), baseline: is_baseline, ..Default::default() };
    for (id, entry) in &new {
        match old.get(id) {
            None => diff.added.push(change(*id, None, None)),
            Some(previous) if previous.file_id != entry.file_id => diff.updated.push(change(*id, Some(display(previous)), Some(display(entry)))),
            Some(_) => {}
        }
    }
    for id in old.keys().filter(|id| !new.contains_key(id)) {
        diff.removed.push(change(*id, None, None));
    }
    for list in [&mut diff.added, &mut diff.updated, &mut diff.removed] {
        list.sort_by_key(|c| c.name.to_lowercase());
    }
    Ok(diff)
}

pub fn render(notes: &str, diff: &ModDiff) -> String {
    let mut out = notes.trim_end().to_string();
    let link = |c: &ModChange| match &c.url {
        Some(url) => format!("[{}]({url})", c.name),
        None => c.name.clone(),
    };
    if diff.added.is_empty() && diff.updated.is_empty() && diff.removed.is_empty() {
        return out;
    }
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str("## Mod changes\n");
    if !diff.added.is_empty() {
        out.push_str("\n### Added\n");
        for c in &diff.added {
            let _ = writeln!(out, "- {}", link(c));
        }
    }
    if !diff.updated.is_empty() {
        out.push_str("\n### Updated\n");
        for c in &diff.updated {
            let _ = writeln!(out, "- {}: {} to {}", link(c), c.from.as_deref().unwrap_or("?"), c.to.as_deref().unwrap_or("?"));
        }
    }
    if !diff.removed.is_empty() {
        out.push_str("\n### Removed\n");
        for c in &diff.removed {
            let _ = writeln!(out, "- {}", link(c));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn reads_curseforge_manifest_from_zip() {
        let path = std::env::temp_dir().join("packraven-baseline-test.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("manifest.json", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(br#"{"name":"Pack","version":"1.2.0","files":[{"projectID":10,"fileID":20,"required":true}]}"#).unwrap();
        zip.finish().unwrap();
        let baseline = baseline_from_zip(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(baseline.label, "Pack 1.2.0");
        assert_eq!(baseline.lock.files[0].project_id, 10);
        assert_eq!(baseline.lock.files[0].file_id, 20);
    }
}
