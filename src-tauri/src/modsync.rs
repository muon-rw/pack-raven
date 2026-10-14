use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};
use futures_util::{stream, StreamExt};
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use sha1::{Digest, Sha1};

use crate::curseforge;
use crate::git;
use crate::http;
use crate::jobs::Log;
use crate::pack::{LockEntry, Lockfile, LOCKFILE};

const FOLDERS: [&str; 4] = ["mods", "resourcepacks", "shaderpacks", "datapacks"];
const EXTENSIONS: [&str; 3] = [".jar", ".zip", ".disabled"];
const LOCAL_BUILD_SUFFIX: &str = "-dev.jar";
const INSTANCE_JSON: &str = "minecraftinstance.json";
const CACHE_MAX_AGE: Duration = Duration::from_secs(60 * 86400);
const PARALLEL_DOWNLOADS: usize = 6;

type FileSource = (u64, u64, String);

pub fn managed_files(root: &Path) -> Result<Vec<String>> {
    let tracked: HashSet<String> = git::tracked_files(root)?.into_iter().collect();
    let mut found = Vec::new();
    for folder in FOLDERS {
        let Ok(entries) = std::fs::read_dir(root.join(folder)) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = format!("{folder}/{name}");
            let managed = EXTENSIONS.iter().any(|e| name.ends_with(e)) && !name.ends_with(LOCAL_BUILD_SUFFIX) && !tracked.contains(&path);
            if managed && entry.metadata().is_ok_and(|m| m.is_file()) {
                found.push(path);
            }
        }
    }
    Ok(found)
}

struct Repo {
    root: PathBuf,
    state_dir: PathBuf,
}

impl Repo {
    fn open(dir: &Path) -> Result<Self> {
        let root = PathBuf::from(git::run(dir, &["rev-parse", "--show-toplevel"])?.trim());
        let git_dir = PathBuf::from(git::run(dir, &["rev-parse", "--absolute-git-dir"])?.trim());
        Ok(Self { root, state_dir: git_dir.join("modsync") })
    }

    fn cache_dir(&self) -> PathBuf {
        self.state_dir.join("cache")
    }

    fn cached(&self, sha1: &str) -> PathBuf {
        self.cache_dir().join(sha1)
    }

    fn read_lock(&self) -> Result<Lockfile> {
        match std::fs::read_to_string(self.root.join(LOCKFILE)) {
            Ok(text) => Lockfile::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Lockfile::default()),
            Err(e) => Err(e.into()),
        }
    }

    fn write_lock(&self, lock: &mut Lockfile, stage: bool) -> Result<()> {
        lock.files.sort_by_key(|e| e.path.to_lowercase());
        let mut text = String::from("{\n");
        for (key, value) in [("minecraft", &lock.minecraft), ("loader", &lock.loader)] {
            if let Some(value) = value.as_deref().filter(|v| !v.is_empty()) {
                text += &format!("  \"{key}\": {},\n", serde_json::to_string(value)?);
            }
        }
        text += "  \"files\": [\n";
        for (i, entry) in lock.files.iter().enumerate() {
            let comma = if i + 1 < lock.files.len() { "," } else { "" };
            text += &format!("    {}{comma}\n", spaced_json(entry)?);
        }
        text += "  ]\n}\n";
        std::fs::write(self.root.join(LOCKFILE), text)?;
        if stage {
            git::run(&self.root, &["add", LOCKFILE])?;
        }
        Ok(())
    }
}

// Matches Python's json.dumps spacing, which earlier lockfiles used, so we don't rewrite every line.
fn spaced_json(value: &impl Serialize) -> Result<String> {
    struct Spaced;
    impl serde_json::ser::Formatter for Spaced {
        fn begin_object_key<W: ?Sized + Write>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()> {
            if first { Ok(()) } else { writer.write_all(b", ") }
        }

        fn begin_object_value<W: ?Sized + Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
            writer.write_all(b": ")
        }
    }
    let mut out = Vec::new();
    value.serialize(&mut serde_json::Serializer::with_formatter(&mut out, Spaced))?;
    Ok(String::from_utf8(out)?)
}

struct HashCache {
    path: PathBuf,
    entries: HashMap<String, (u64, u64, String)>,
    dirty: bool,
}

impl HashCache {
    fn load(repo: &Repo) -> Self {
        let path = repo.state_dir.join("hashes.json");
        let entries = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Self { path, entries, dirty: false }
    }

    fn sha1(&mut self, root: &Path, path: &str) -> Result<String> {
        let meta = std::fs::metadata(root.join(path))?;
        let mtime = meta.modified()?.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        if let Some((size, cached_mtime, digest)) = self.entries.get(path)
            && *size == meta.len()
            && *cached_mtime == mtime
        {
            return Ok(digest.clone());
        }
        let digest = sha1_of(&root.join(path))?;
        self.entries.insert(path.to_string(), (meta.len(), mtime, digest.clone()));
        self.dirty = true;
        Ok(digest)
    }

    fn forget(&mut self, path: &str) {
        self.dirty |= self.entries.remove(path).is_some();
    }

    fn save(&self) -> Result<()> {
        if self.dirty {
            std::fs::create_dir_all(self.path.parent().unwrap())?;
            std::fs::write(&self.path, serde_json::to_string(&self.entries)?)?;
        }
        Ok(())
    }
}

fn sha1_of(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(hex::encode(hasher.finalize()));
        }
        hasher.update(&buffer[..read]);
    }
}

// CurseForge's fingerprint: MurmurHash2 with seed 1 over the file minus all whitespace bytes.
fn cf_fingerprint(bytes: &[u8]) -> u32 {
    let data: Vec<u8> = bytes.iter().copied().filter(|b| !matches!(b, b'\t' | b'\n' | b'\r' | b' ')).collect();
    const M: u32 = 0x5BD1_E995;
    let mut h = 1 ^ data.len() as u32;
    let mut chunks = data.chunks_exact(4);
    for chunk in &mut chunks {
        let mut k = u32::from_le_bytes(chunk.try_into().unwrap()).wrapping_mul(M);
        k ^= k >> 24;
        h = h.wrapping_mul(M) ^ k.wrapping_mul(M);
    }
    let tail = chunks.remainder();
    if !tail.is_empty() {
        for (i, byte) in tail.iter().enumerate().rev() {
            h ^= u32::from(*byte) << (8 * i);
        }
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^ (h >> 15)
}

fn cdn_url(file_id: u64, file_name: &str) -> String {
    let mut encoded = String::new();
    for byte in file_name.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded += &format!("%{byte:02X}");
        }
    }
    format!("https://edge.forgecdn.net/files/{}/{}/{encoded}", file_id / 1000, file_id % 1000)
}

struct Instance {
    by_sha1: HashMap<String, FileSource>,
    minecraft: Option<String>,
    loader: Option<String>,
}

fn read_instance(root: &Path) -> Instance {
    let instance: Value = std::fs::read_to_string(root.join(INSTANCE_JSON))
        .ok()
        .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default();
    let mut by_sha1 = HashMap::new();
    for addon in instance["installedAddons"].as_array().into_iter().flatten() {
        let installed = &addon["installedFile"];
        let sha1 = installed["hashes"].as_array().into_iter().flatten().find(|h| h["type"] == 1).and_then(|h| h["value"].as_str());
        let (Some(sha1), Some(project_id), Some(file_id)) = (sha1, addon["addonID"].as_u64(), installed["id"].as_u64()) else { continue };
        let url = match installed["downloadUrl"].as_str().filter(|u| !u.is_empty()) {
            Some(url) => url.to_string(),
            None => cdn_url(file_id, installed["fileName"].as_str().unwrap_or_default()),
        };
        by_sha1.insert(sha1.to_string(), (project_id, file_id, url));
    }
    Instance {
        by_sha1,
        minecraft: instance["gameVersion"].as_str().map(String::from),
        loader: instance.pointer("/baseModLoader/name").and_then(Value::as_str).map(String::from),
    }
}

async fn resolve_by_fingerprint(root: &Path, paths: &[String]) -> Result<HashMap<String, FileSource>> {
    if curseforge::core_key().is_none() {
        return Ok(HashMap::new());
    }
    let mut by_fingerprint = HashMap::new();
    for path in paths {
        by_fingerprint.insert(cf_fingerprint(&std::fs::read(root.join(path))?), path.clone());
    }
    let fingerprints: Vec<u32> = by_fingerprint.keys().copied().collect();
    let mut resolved = HashMap::new();
    for found in curseforge::fingerprint_matches(&fingerprints).await? {
        if let Some(path) = by_fingerprint.get(&found.file.file_fingerprint) {
            let url = found.file.download_url.unwrap_or_else(|| cdn_url(found.file.id, &found.file.file_name));
            resolved.insert(path.clone(), (found.id, found.file.id, url));
        }
    }
    Ok(resolved)
}

async fn build_lock(repo: &Repo) -> Result<(Lockfile, Vec<String>)> {
    let mut hashes = HashCache::load(repo);
    let old = repo.read_lock()?;
    let instance = read_instance(&repo.root);
    let mut known: HashMap<String, FileSource> = old.files.iter().map(|e| (e.sha1.clone(), (e.project_id, e.file_id, e.url.clone()))).collect();
    known.extend(instance.by_sha1);

    let mut files = Vec::new();
    let mut unknown = Vec::new();
    for path in managed_files(&repo.root)? {
        let sha1 = hashes.sha1(&repo.root, &path)?;
        match known.get(&sha1) {
            Some(source) => files.push((path, sha1, source.clone())),
            None => unknown.push(path),
        }
    }
    if !unknown.is_empty() {
        for (path, source) in resolve_by_fingerprint(&repo.root, &unknown).await? {
            let sha1 = hashes.sha1(&repo.root, &path)?;
            unknown.retain(|p| *p != path);
            files.push((path, sha1, source));
        }
    }
    hashes.save()?;

    let mut entries = Vec::new();
    for (path, sha1, (project_id, file_id, url)) in files {
        let size = std::fs::metadata(repo.root.join(&path))?.len();
        entries.push(LockEntry { path, project_id, file_id, sha1, size, url });
    }
    let lock = Lockfile { minecraft: instance.minecraft.or(old.minecraft), loader: instance.loader.or(old.loader), files: entries };
    unknown.sort();
    Ok((lock, unknown))
}

pub async fn lock(dir: &Path, log: &Log) -> Result<()> {
    let repo = Repo::open(dir)?;
    let (mut lock, unknown) = build_lock(&repo).await?;
    if !unknown.is_empty() {
        log.stream("err", "modsync: these files are not on CurseForge, so you must commit them directly (git add -f <path>) or remove them:");
        for path in &unknown {
            log.stream("err", &format!("  {path}"));
        }
        if curseforge::core_key().is_none() {
            log.stream("err", "modsync: packraven has no CurseForge API key, so modsync skipped the CurseForge fingerprint lookup.");
        }
        bail!("{} file(s) are not on CurseForge", unknown.len());
    }
    repo.write_lock(&mut lock, true)
}

// Unlike the pre-commit hook's lock, we leave mods.lock.json unstaged, so you review and stage it in Git.
pub async fn update_mod_list(dir: &Path) -> Result<()> {
    let repo = Repo::open(dir)?;
    let (mut lock, unknown) = build_lock(&repo).await?;
    match unknown.len() {
        0 => repo.write_lock(&mut lock, false),
        1 => bail!("{} isn't on CurseForge, so the mod list can't record it. Add it to git directly, or remove it.", unknown[0]),
        n => bail!("{n} files aren't on CurseForge, so the mod list can't record them. Add them to git directly, or remove them: {}", unknown.join(", ")),
    }
}

pub async fn folder_lock(dir: &Path) -> Result<Lockfile> {
    Ok(build_lock(&Repo::open(dir)?).await?.0)
}

#[derive(Serialize)]
pub struct FolderUpdate {
    pub from: String,
    pub to: String,
}

#[derive(Serialize, Default)]
pub struct FolderChanges {
    pub updated: Vec<FolderUpdate>,
    pub added: Vec<String>,
    pub missing: Vec<String>,
}

impl FolderChanges {
    pub fn is_empty(&self) -> bool {
        self.updated.is_empty() && self.added.is_empty() && self.missing.is_empty()
    }
}

// The shell polls this, so we compare sizes first and only hash the files that differ. We pair
// those by CurseForge project, so an update reads as one change rather than a new file plus a
// missing one.
pub fn folder_changes(dir: &Path, lock: &Lockfile) -> Result<FolderChanges> {
    let on_disk: HashMap<String, u64> =
        managed_files(dir)?.into_iter().filter_map(|path| std::fs::metadata(dir.join(&path)).ok().map(|m| (path, m.len()))).collect();
    let listed: HashSet<&str> = lock.files.iter().map(|e| e.path.as_str()).collect();
    let mut changes = FolderChanges::default();
    let mut gone: Vec<&LockEntry> = Vec::new();
    for entry in lock.files.iter().filter(|e| on_disk.get(&e.path) != Some(&e.size)) {
        if on_disk.contains_key(&entry.path) {
            changes.updated.push(FolderUpdate { from: entry.path.clone(), to: entry.path.clone() });
        } else {
            gone.push(entry);
        }
    }
    let mut unlisted: Vec<String> = on_disk.into_keys().filter(|p| !listed.contains(p.as_str())).collect();
    unlisted.sort();
    if !unlisted.is_empty() && !gone.is_empty() {
        let repo = Repo::open(dir)?;
        let mut hashes = HashCache::load(&repo);
        let instance = read_instance(&repo.root);
        let mut gone_by_project: HashMap<u64, String> = gone.iter().map(|e| (e.project_id, e.path.clone())).collect();
        for path in std::mem::take(&mut unlisted) {
            let project = instance.by_sha1.get(&hashes.sha1(&repo.root, &path)?).map(|source| source.0);
            match project.and_then(|id| gone_by_project.remove(&id)) {
                Some(from) => changes.updated.push(FolderUpdate { from, to: path }),
                None => unlisted.push(path),
            }
        }
        hashes.save()?;
        gone.retain(|e| gone_by_project.contains_key(&e.project_id));
    }
    changes.added = unlisted;
    changes.missing = gone.into_iter().map(|e| e.path.clone()).collect();
    changes.updated.sort_by_key(|u| u.to.to_lowercase());
    Ok(changes)
}

pub async fn adopt(dir: &Path, log: &Log) -> Result<Vec<String>> {
    let repo = Repo::open(dir)?;
    let (mut lock, unknown) = build_lock(&repo).await?;
    for path in &unknown {
        git::run(&repo.root, &["add", "-f", "--", path])?;
        log.info(format!("modsync: not on CurseForge, committing directly: {path}"));
    }
    repo.write_lock(&mut lock, true)?;
    Ok(unknown)
}

struct SyncPlan {
    missing: Vec<LockEntry>,
    extra: Vec<(String, String)>,
}

fn sync_plan(repo: &Repo, lock: &Lockfile, hashes: &mut HashCache) -> Result<SyncPlan> {
    let wanted: HashMap<&str, &LockEntry> = lock.files.iter().map(|e| (e.path.as_str(), e)).collect();
    let mut present = HashMap::new();
    for path in managed_files(&repo.root)? {
        let sha1 = hashes.sha1(&repo.root, &path)?;
        present.insert(path, sha1);
    }
    let missing = lock.files.iter().filter(|e| present.get(&e.path) != Some(&e.sha1)).cloned().collect();
    let extra = present.into_iter().filter(|(path, sha1)| wanted.get(path.as_str()).is_none_or(|e| e.sha1 != *sha1)).collect();
    Ok(SyncPlan { missing, extra })
}

pub async fn restore(dir: &Path, log: &Log) -> Result<()> {
    let repo = Repo::open(dir)?;
    let lock = repo.read_lock()?;
    if let (Some(local), Some(expected)) = (read_instance(&repo.root).loader, &lock.loader)
        && local != *expected
    {
        log.stream("err", &format!("modsync: WARNING: lockfile expects {expected} but this profile uses {local}; change it in the CurseForge app"));
    }
    let mut hashes = HashCache::load(&repo);
    let plan = sync_plan(&repo, &lock, &mut hashes)?;
    if plan.missing.is_empty() && plan.extra.is_empty() {
        return hashes.save();
    }
    std::fs::create_dir_all(repo.cache_dir())?;

    for (path, sha1) in &plan.extra {
        let cached = repo.cached(sha1);
        if cached.exists() {
            std::fs::remove_file(repo.root.join(path))?;
        } else {
            std::fs::rename(repo.root.join(path), cached)?;
        }
        hashes.forget(path);
        log.info(format!("modsync: removed {path}"));
    }

    let mut to_download: HashMap<&str, &LockEntry> = HashMap::new();
    for entry in plan.missing.iter().filter(|e| !repo.cached(&e.sha1).exists()) {
        to_download.insert(&entry.sha1, entry);
    }
    let mut failed = HashSet::new();
    if !to_download.is_empty() {
        log.info(format!("modsync: downloading {} file(s)", to_download.len()));
        let results: Vec<_> = stream::iter(to_download.into_values())
            .map(|entry| {
                let target = repo.cached(&entry.sha1);
                async move { (entry, http::download(&entry.url, &target, Some(&entry.sha1)).await) }
            })
            .buffer_unordered(PARALLEL_DOWNLOADS)
            .collect()
            .await;
        for (entry, result) in results {
            if let Err(e) = result {
                log.stream("err", &format!("modsync: failed {}: {e:#}", entry.path));
                failed.insert(entry.sha1.clone());
            }
        }
    }

    for entry in plan.missing.iter().filter(|e| !failed.contains(&e.sha1)) {
        let source = repo.cached(&entry.sha1);
        let target = repo.root.join(&entry.path);
        std::fs::create_dir_all(target.parent().unwrap())?;
        std::fs::copy(&source, target)?;
        std::fs::File::options().write(true).open(&source)?.set_modified(SystemTime::now())?;
        log.info(format!("modsync: placed {}", entry.path));
    }

    expire_cache(&repo.cache_dir())?;
    hashes.save()?;
    if !failed.is_empty() {
        bail!("{} file(s) failed to download; pull again or run `sh .githooks/run.sh restore` to retry", failed.len());
    }
    Ok(())
}

fn expire_cache(cache_dir: &Path) -> Result<()> {
    let cutoff = SystemTime::now() - CACHE_MAX_AGE;
    for entry in std::fs::read_dir(cache_dir)?.flatten() {
        if entry.metadata()?.modified()? < cutoff {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

pub fn status(dir: &Path, log: &Log) -> Result<()> {
    let repo = Repo::open(dir)?;
    let mut hashes = HashCache::load(&repo);
    let plan = sync_plan(&repo, &repo.read_lock()?, &mut hashes)?;
    hashes.save()?;
    for entry in &plan.missing {
        log.info(format!("missing  {}", entry.path));
    }
    for (path, _) in &plan.extra {
        log.info(format!("unlocked {path}"));
    }
    if plan.missing.is_empty() && plan.extra.is_empty() {
        log.info("in sync");
    }
    Ok(())
}

pub fn iris_clean(input: &str) -> String {
    let timestamp = Regex::new(r"^#\w{3} \w{3} \d{1,2} \d{2}:\d{2}:\d{2} \w+ \d{4}\s*$").expect("valid regex");
    let mut out = String::new();
    for line in input.lines().filter(|l| !timestamp.is_match(l)) {
        out += if line.starts_with("enableShaders=") { "enableShaders=false" } else { line };
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cdn_url_escapes_like_python() {
        assert_eq!(cdn_url(4_567_890, "Mod Name+1.2 [x].jar"), "https://edge.forgecdn.net/files/4567/890/Mod%20Name%2B1.2%20%5Bx%5D.jar");
    }

    #[test]
    fn fingerprint_matches_python_reference() {
        let all_bytes: Vec<u8> = (0..=255).collect();
        let cases: [(&[u8], u32); 6] = [
            (b"", 1540447798),
            (b"a", 626045324),
            (b"ab c", 1621425345),
            (b"abc\n", 1621425345),
            (b"hello world, packraven!\t\r\n", 38433397),
            (&all_bytes, 2094645347),
        ];
        for (data, expected) in cases {
            assert_eq!(cf_fingerprint(data), expected, "{data:?}");
        }
    }

    #[test]
    fn lock_entries_keep_python_spacing() {
        let entry = LockEntry { path: "mods/a.jar".into(), project_id: 1, file_id: 2, sha1: "ab".into(), size: 3, url: "u".into() };
        assert_eq!(spaced_json(&entry).unwrap(), r#"{"path": "mods/a.jar", "projectId": 1, "fileId": 2, "sha1": "ab", "size": 3, "url": "u"}"#);
    }

    #[test]
    fn iris_clean_drops_timestamp_and_disables_shaders() {
        let input = "#Mon Oct 13 18:58:55 CEST 2026\r\nenableShaders=true\r\nshaderPack=x.zip\r\n";
        assert_eq!(iris_clean(input), "enableShaders=false\nshaderPack=x.zip\n");
    }
}
