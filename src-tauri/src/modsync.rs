use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
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
const NOT_ON_CURSEFORGE_RECHECK: Duration = Duration::from_secs(86400);
// A jar you haven't committed may be one you just uploaded, and CurseForge takes 5 to 10 minutes to
// approve a file, so in the background we ask again about these far sooner.
const UNCOMMITTED_RECHECK: Duration = Duration::from_secs(60);
const PARALLEL_DOWNLOADS: usize = 6;
const DOWNLOAD_ATTEMPTS: usize = 3;

type FileSource = (u64, u64, String);

pub fn is_binary(relative: &str) -> bool {
    FOLDERS.iter().any(|folder| {
        relative.strip_prefix(folder).and_then(|rest| rest.strip_prefix('/')).is_some_and(|name| !name.contains('/') && EXTENSIONS.iter().any(|e| name.ends_with(e)))
    })
}

// Mods write their own files into subfolders of mods/ while the game runs, and the mod list only
// records the folders' top-level files, so only those trigger a refresh.
pub fn affects_mod_list(relative: &str) -> bool {
    relative == INSTANCE_JSON || is_binary(relative)
}

pub fn managed_files(root: &Path) -> Result<Vec<String>> {
    Ok(binary_files(root)?.untracked)
}

struct BinaryFiles {
    untracked: Vec<String>,
    committed: Vec<String>,
}

fn binary_files(root: &Path) -> Result<BinaryFiles> {
    let tracked: HashSet<String> = git::tracked_files(root)?.into_iter().collect();
    let mut found = BinaryFiles { untracked: Vec::new(), committed: Vec::new() };
    for folder in FOLDERS {
        let Ok(entries) = std::fs::read_dir(root.join(folder)) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = format!("{folder}/{name}");
            if !EXTENSIONS.iter().any(|e| name.ends_with(e)) || name.ends_with(LOCAL_BUILD_SUFFIX) || !entry.metadata().is_ok_and(|m| m.is_file()) {
                continue;
            }
            if tracked.contains(&path) { found.committed.push(path) } else { found.untracked.push(path) }
        }
    }
    Ok(found)
}

struct Repo {
    root: PathBuf,
    state_dir: PathBuf,
}

// Every mod list refresh opens the repo, and asking git costs two process starts, so we remember
// where each pack's repo is for as long as its git dir exists.
static REPOS: std::sync::Mutex<Option<HashMap<PathBuf, (PathBuf, PathBuf)>>> = std::sync::Mutex::new(None);

impl Repo {
    fn open(dir: &Path) -> Result<Self> {
        let cached = REPOS.lock().unwrap().as_ref().and_then(|repos| repos.get(dir).cloned()).filter(|(_, git_dir)| git_dir.is_dir());
        let (root, git_dir) = match cached {
            Some(found) => found,
            None => {
                let root = PathBuf::from(git::run(dir, &["rev-parse", "--show-toplevel"])?.trim());
                let git_dir = PathBuf::from(git::run(dir, &["rev-parse", "--absolute-git-dir"])?.trim());
                REPOS.lock().unwrap().get_or_insert_default().insert(dir.to_path_buf(), (root.clone(), git_dir.clone()));
                (root, git_dir)
            }
        };
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

    /// Returns whether the lockfile's contents changed.
    fn write_lock(&self, lock: &mut Lockfile, stage: bool) -> Result<bool> {
        let text = render_lock(lock)?;
        // The watcher reports every write, so we skip identical ones to avoid a refresh loop. Without
        // `eol=lf` in .gitattributes, git may check the lockfile out with CRLF, which still counts as identical.
        let changed = std::fs::read_to_string(self.root.join(LOCKFILE)).ok().map(|existing| existing.replace("\r\n", "\n")).as_deref() != Some(text.as_str());
        if changed {
            std::fs::write(self.root.join(LOCKFILE), text)?;
        }
        if stage {
            git::run(&self.root, &["add", LOCKFILE])?;
        }
        Ok(changed)
    }

    fn restoring_marker(&self) -> PathBuf {
        self.state_dir.join("restoring")
    }
}

fn render_lock(lock: &mut Lockfile) -> Result<String> {
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
    Ok(text)
}

// We hold this while rewriting mods.lock.json or the mods folder, so the automatic mod list update
// can't run in the middle of a pull, restore or publish.
pub static MOD_LIST_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const RESTORE_MARKER_MAX_AGE: Duration = Duration::from_secs(15 * 60);

struct RestoringMarker(PathBuf);

impl RestoringMarker {
    fn create(repo: &Repo) -> Result<Self> {
        std::fs::create_dir_all(&repo.state_dir)?;
        std::fs::write(repo.restoring_marker(), std::process::id().to_string())?;
        Ok(Self(repo.restoring_marker()))
    }
}

impl Drop for RestoringMarker {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// The git hooks restore in their own process, so packraven can't use MOD_LIST_GUARD to wait for them.
// A marker older than RESTORE_MARKER_MAX_AGE belongs to a restore that crashed.
pub fn restore_in_progress(dir: &Path) -> bool {
    let Ok(repo) = Repo::open(dir) else { return false };
    std::fs::metadata(repo.restoring_marker())
        .and_then(|m| m.modified())
        .is_ok_and(|modified| modified.elapsed().is_ok_and(|age| age < RESTORE_MARKER_MAX_AGE))
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

// We ask CurseForge about every jar the mod list can't place, in case it has since been published
// there. Remembering the misses keeps that request, and a full read of each jar, off most background
// refreshes.
struct NotOnCurseforge {
    path: PathBuf,
    checked_at: HashMap<String, u64>,
    dirty: bool,
}

impl NotOnCurseforge {
    fn load(repo: &Repo) -> Self {
        let path = repo.state_dir.join("not-on-curseforge.json");
        let mut checked_at: HashMap<String, u64> = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let loaded = checked_at.len();
        let now = unix_now();
        checked_at.retain(|_, at| now.saturating_sub(*at) < NOT_ON_CURSEFORGE_RECHECK.as_secs());
        Self { path, dirty: checked_at.len() != loaded, checked_at }
    }

    fn checked_within(&self, sha1: &str, max_age: Duration) -> bool {
        self.checked_at.get(sha1).is_some_and(|at| unix_now().saturating_sub(*at) < max_age.as_secs())
    }

    fn insert(&mut self, sha1: String) {
        self.checked_at.insert(sha1, unix_now());
        self.dirty = true;
    }

    fn save(&self) -> Result<()> {
        if self.dirty {
            std::fs::create_dir_all(self.path.parent().unwrap())?;
            std::fs::write(&self.path, serde_json::to_string(&self.checked_at)?)?;
        }
        Ok(())
    }
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
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

pub fn cdn_url(file_id: u64, file_name: &str) -> String {
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

struct BuiltLock {
    lock: Lockfile,
    unknown: Vec<String>,
    /// Committed directly but now on CurseForge. `lock` already records these, so the caller must untrack them.
    now_on_curseforge: Vec<String>,
}

/// How to treat a jar you haven't committed that CurseForge didn't recognize last time we asked.
#[derive(Clone, Copy)]
pub enum Lookup {
    /// Ask again. Publish, pull and commits stop on these jars, so they never trust an old answer.
    Fresh,
    /// Ask again once UNCOMMITTED_RECHECK has passed.
    Cached,
}

async fn build_lock(repo: &Repo, lookup: Lookup) -> Result<BuiltLock> {
    let mut hashes = HashCache::load(repo);
    let mut not_on_curseforge = NotOnCurseforge::load(repo);
    let old = repo.read_lock()?;
    let instance = read_instance(&repo.root);
    let mut known: HashMap<String, FileSource> = old.files.iter().map(|e| (e.sha1.clone(), (e.project_id, e.file_id, e.url.clone()))).collect();
    known.extend(instance.by_sha1);

    let binaries = binary_files(&repo.root)?;
    let mut files = Vec::new();
    let mut unknown = Vec::new();
    let mut unknown_unchecked = Vec::new();
    for path in binaries.untracked {
        let sha1 = hashes.sha1(&repo.root, &path)?;
        match known.get(&sha1) {
            Some(source) => files.push((path, sha1, source.clone())),
            None if matches!(lookup, Lookup::Cached) && not_on_curseforge.checked_within(&sha1, UNCOMMITTED_RECHECK) => unknown.push(path),
            None => unknown_unchecked.push(path),
        }
    }

    // git keeps tracking a jar you committed directly even after it's published on CurseForge, since
    // .gitignore only applies to untracked files. We move those jars into the mod list.
    let mut now_on_curseforge = Vec::new();
    let mut committed_unchecked = Vec::new();
    for path in binaries.committed {
        let sha1 = hashes.sha1(&repo.root, &path)?;
        match known.get(&sha1) {
            Some(source) => {
                files.push((path.clone(), sha1, source.clone()));
                now_on_curseforge.push(path);
            }
            None if !not_on_curseforge.checked_within(&sha1, NOT_ON_CURSEFORGE_RECHECK) => committed_unchecked.push(path),
            None => {}
        }
    }

    let lookup: Vec<String> = unknown_unchecked.iter().chain(&committed_unchecked).cloned().collect();
    let resolved = if lookup.is_empty() || curseforge::core_key().is_none() {
        None
    } else {
        match resolve_by_fingerprint(&repo.root, &lookup).await {
            Ok(resolved) => Some(resolved),
            // Jars you committed directly don't need a lock entry, so a failed lookup for only those
            // shouldn't fail the whole refresh. We'll ask again next time.
            Err(_) if unknown_unchecked.is_empty() => None,
            Err(e) => return Err(e),
        }
    };
    match resolved {
        Some(mut resolved) => {
            for path in lookup {
                let sha1 = hashes.sha1(&repo.root, &path)?;
                let Some(source) = resolved.remove(&path) else {
                    not_on_curseforge.insert(sha1);
                    if unknown_unchecked.contains(&path) {
                        unknown.push(path);
                    }
                    continue;
                };
                if committed_unchecked.contains(&path) {
                    now_on_curseforge.push(path.clone());
                }
                files.push((path, sha1, source));
            }
        }
        None => unknown.extend(unknown_unchecked),
    }
    hashes.save()?;
    not_on_curseforge.save()?;

    let mut entries = Vec::new();
    for (path, sha1, (project_id, file_id, url)) in files {
        let size = std::fs::metadata(repo.root.join(&path))?.len();
        entries.push(LockEntry { path, project_id, file_id, sha1, size, url });
    }
    let lock = Lockfile { minecraft: instance.minecraft.or(old.minecraft), loader: instance.loader.or(old.loader), files: entries };
    unknown.sort();
    now_on_curseforge.sort();
    Ok(BuiltLock { lock, unknown, now_on_curseforge })
}

fn untrack(root: &Path, paths: &[String]) -> Result<()> {
    for chunk in paths.chunks(100) {
        let mut args = vec!["rm", "--cached", "--quiet", "--"];
        args.extend(chunk.iter().map(String::as_str));
        git::run(root, &args)?;
    }
    Ok(())
}

fn log_now_on_curseforge(log: &Log, paths: &[String]) {
    for path in paths {
        log.info(format!("modsync: now on CurseForge, moving it from git into the mod list: {path}"));
    }
}

pub async fn lock(dir: &Path, log: &Log) -> Result<()> {
    let repo = Repo::open(dir)?;
    let mut built = build_lock(&repo, Lookup::Fresh).await?;
    if !built.unknown.is_empty() {
        log.stream("err", "modsync: these files are not on CurseForge, so you must commit them directly (git add -f <path>) or remove them:");
        for path in &built.unknown {
            log.stream("err", &format!("  {path}"));
        }
        if curseforge::core_key().is_none() {
            log.stream("err", &format!("modsync: {}, so we skipped the CurseForge fingerprint lookup.", curseforge::NO_CORE_KEY));
        }
        bail!("{} file(s) are not on CurseForge", built.unknown.len());
    }
    repo.write_lock(&mut built.lock, true)?;
    untrack(&repo.root, &built.now_on_curseforge)?;
    log_now_on_curseforge(log, &built.now_on_curseforge);
    Ok(())
}

pub async fn refresh_mod_list(dir: &Path) -> Result<Vec<String>> {
    Ok(refresh(dir, Lookup::Fresh).await?.unknown)
}

pub struct Refresh {
    pub unknown: Vec<String>,
    pub changed: bool,
}

// Unlike the pre-commit hook's lock, we leave mods.lock.json unstaged. Untracking a jar that's now on
// CurseForge does stage its removal, since git has no unstaged form of that. We record every file we
// can resolve and return the rest, so one unknown jar doesn't hold back the whole list.
pub async fn refresh(dir: &Path, lookup: Lookup) -> Result<Refresh> {
    let repo = Repo::open(dir)?;
    let mut built = build_lock(&repo, lookup).await?;
    let changed = repo.write_lock(&mut built.lock, false)?;
    untrack(&repo.root, &built.now_on_curseforge)?;
    Ok(Refresh { unknown: built.unknown, changed: changed || !built.now_on_curseforge.is_empty() })
}

pub async fn update_mod_list(dir: &Path) -> Result<()> {
    let unknown = refresh_mod_list(dir).await?;
    match unknown.len() {
        0 => Ok(()),
        1 => bail!("{} isn't on CurseForge, so the mod list can't record it. Commit it directly, or remove it.", unknown[0]),
        n => bail!("{n} files aren't on CurseForge, so the mod list can't record them. Commit them directly, or remove them: {}", unknown.join(", ")),
    }
}

pub fn lock_at(dir: &Path, rev: &str) -> Result<Lockfile> {
    git::show(dir, rev, LOCKFILE).map_or_else(|| Ok(Lockfile::default()), |text| Lockfile::parse(&text))
}

pub fn write_lock(dir: &Path, lock: &mut Lockfile) -> Result<()> {
    Repo::open(dir)?.write_lock(lock, false)?;
    Ok(())
}

pub fn track_directly(dir: &Path, paths: &[String]) -> Result<()> {
    let mut args = vec!["add", "-f", "--"];
    args.extend(paths.iter().map(String::as_str));
    git::run(dir, &args)?;
    Ok(())
}

#[derive(Serialize, Default, Debug, PartialEq)]
pub struct ModCounts {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
}

impl ModCounts {
    pub fn is_empty(&self) -> bool {
        self.added + self.updated + self.removed == 0
    }
}

pub fn mod_counts(from: &Lockfile, to: &Lockfile) -> ModCounts {
    let keys = MergeKeys::new(&[from, to]);
    let (before, after) = (keys.index(from), keys.index(to));
    let mut counts = ModCounts::default();
    for (key, entry) in &after {
        match before.get(key) {
            None => counts.added += 1,
            Some(old) if old.sha1 != entry.sha1 => counts.updated += 1,
            _ => {}
        }
    }
    counts.removed = before.keys().filter(|key| !after.contains_key(*key)).count();
    counts
}

#[derive(Clone, Copy, Debug, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Prefer {
    Mine,
    Theirs,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct ModConflict {
    pub name: String,
    pub mine: Option<String>,
    pub theirs: Option<String>,
}

// We match entries by CurseForge project, so updating a mod on one side and leaving it alone on the
// other merges cleanly even though the path changed. Packs can hold several files of one project
// (resource pack variants, say), and those we match by path instead.
struct MergeKeys {
    by_path: HashSet<u64>,
}

impl MergeKeys {
    fn new(locks: &[&Lockfile]) -> Self {
        let mut by_path = HashSet::new();
        for lock in locks {
            let mut seen = HashSet::new();
            for entry in &lock.files {
                if entry.project_id == 0 || !seen.insert(entry.project_id) {
                    by_path.insert(entry.project_id);
                }
            }
        }
        Self { by_path }
    }

    fn key(&self, entry: &LockEntry) -> String {
        if self.by_path.contains(&entry.project_id) { format!("path:{}", entry.path) } else { format!("project:{}", entry.project_id) }
    }

    fn index<'a>(&self, lock: &'a Lockfile) -> HashMap<String, &'a LockEntry> {
        lock.files.iter().map(|e| (self.key(e), e)).collect()
    }
}

fn file_name(entry: Option<&LockEntry>) -> Option<String> {
    entry.map(|e| e.path.rsplit('/').next().unwrap_or(&e.path).to_string())
}

fn merge_value(name: &str, base: &Option<String>, mine: &Option<String>, theirs: &Option<String>, prefer: Option<Prefer>, conflicts: &mut Vec<ModConflict>) -> Option<String> {
    if mine == base || mine == theirs {
        return theirs.clone();
    }
    if theirs == base {
        return mine.clone();
    }
    match prefer {
        Some(Prefer::Mine) => mine.clone(),
        Some(Prefer::Theirs) => theirs.clone(),
        None => {
            conflicts.push(ModConflict { name: name.into(), mine: mine.clone(), theirs: theirs.clone() });
            mine.clone()
        }
    }
}

/// Replays your mod changes since `base` onto `theirs`. Without `prefer`, a mod that both sides
/// changed differently is a conflict.
pub fn merge_locks(base: &Lockfile, mine: &Lockfile, theirs: &Lockfile, prefer: Option<Prefer>) -> std::result::Result<Lockfile, Vec<ModConflict>> {
    let keys = MergeKeys::new(&[base, mine, theirs]);
    let (base_files, mine_files, theirs_files) = (keys.index(base), keys.index(mine), keys.index(theirs));
    let mut all: Vec<&String> = base_files.keys().chain(mine_files.keys()).chain(theirs_files.keys()).collect();
    all.sort();
    all.dedup();

    let mut conflicts = Vec::new();
    let mut files = Vec::new();
    for key in all {
        let (b, m, t) = (base_files.get(key).copied(), mine_files.get(key).copied(), theirs_files.get(key).copied());
        let sha = |e: Option<&LockEntry>| e.map(|e| e.sha1.clone());
        let chosen = if sha(m) == sha(b) || sha(m) == sha(t) {
            t
        } else if sha(t) == sha(b) {
            m
        } else {
            match prefer {
                Some(Prefer::Mine) => m,
                Some(Prefer::Theirs) => t,
                None => {
                    let name = file_name(b.or(m).or(t)).unwrap_or_default();
                    conflicts.push(ModConflict { name, mine: file_name(m), theirs: file_name(t) });
                    m
                }
            }
        };
        files.extend(chosen.cloned());
    }
    let minecraft = merge_value("Minecraft version", &base.minecraft, &mine.minecraft, &theirs.minecraft, prefer, &mut conflicts);
    let loader = merge_value("Mod loader", &base.loader, &mine.loader, &theirs.loader, prefer, &mut conflicts);
    if conflicts.is_empty() { Ok(Lockfile { minecraft, loader, files }) } else { Err(conflicts) }
}

pub async fn folder_lock(dir: &Path) -> Result<Lockfile> {
    Ok(build_lock(&Repo::open(dir)?, Lookup::Cached).await?.lock)
}

pub async fn adopt(dir: &Path, log: &Log) -> Result<Vec<String>> {
    let repo = Repo::open(dir)?;
    let mut built = build_lock(&repo, Lookup::Fresh).await?;
    for path in &built.unknown {
        git::run(&repo.root, &["add", "-f", "--", path])?;
        log.info(format!("modsync: not on CurseForge, committing directly: {path}"));
    }
    repo.write_lock(&mut built.lock, true)?;
    untrack(&repo.root, &built.now_on_curseforge)?;
    log_now_on_curseforge(log, &built.now_on_curseforge);
    Ok(built.unknown)
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
    sync_folder(dir, log, true).await
}

// Setup rebuilds mods.lock.json from the folder right after this, so we only fill in files that are
// absent and leave every file you already have in place.
pub async fn download_missing(dir: &Path, log: &Log) -> Result<()> {
    sync_folder(dir, log, false).await
}

pub struct Placement {
    pub entry: LockEntry,
    pub replaces: Option<String>,
}

// The caller holds MOD_LIST_GUARD. We download and check everything before we touch the folder, so
// a failed download leaves the pack as it was. Writing the lock entries ourselves spares the next refresh a
// fingerprint lookup.
pub async fn place(dir: &Path, placements: Vec<Placement>, log: &Log) -> Result<()> {
    let repo = Repo::open(dir)?;
    std::fs::create_dir_all(repo.cache_dir())?;
    let downloads: Vec<LockEntry> = placements.iter().filter(|p| !repo.cached(&p.entry.sha1).exists()).map(|p| p.entry.clone()).collect();
    let results: Vec<Result<()>> = stream::iter(downloads)
        .map(|entry| {
            let target = repo.cached(&entry.sha1);
            async move {
                log.info(format!("Downloading {}", entry.path));
                let mut result = http::download(&entry.url, &target, Some(&entry.sha1)).await;
                // A connection can drop partway through a download, and one failure would otherwise
                // cancel every change in the batch.
                for _ in 1..DOWNLOAD_ATTEMPTS {
                    if result.is_ok() {
                        break;
                    }
                    result = http::download(&entry.url, &target, Some(&entry.sha1)).await;
                }
                result
            }
        })
        .buffer_unordered(PARALLEL_DOWNLOADS)
        .collect()
        .await;
    results.into_iter().collect::<Result<Vec<()>>>().context("a download failed, so packraven left your mods as they were")?;
    let replaced: HashSet<&str> = placements.iter().filter_map(|p| p.replaces.as_deref()).collect();
    if let Some(taken) = placements.iter().find(|p| !replaced.contains(p.entry.path.as_str()) && repo.root.join(&p.entry.path).exists()) {
        bail!("{} already exists", taken.entry.path);
    }

    let mut lock = repo.read_lock()?;
    let mut failed = Vec::new();
    for Placement { entry, replaces } in placements {
        if let Some(old) = replaces {
            let sha1 = lock.files.iter().find(|e| e.path == old).map(|e| e.sha1.clone());
            let removed = match sha1 {
                Some(sha1) => retire(&repo, &old, &sha1),
                None => std::fs::remove_file(repo.root.join(&old)).map_err(Into::into),
            };
            // Minecraft can hold a jar open while it runs. If we placed the new file anyway, the
            // pack would load two versions of the mod, so we leave this one as it was.
            if let Err(error) = removed {
                log.stream("err", &format!("Couldn't remove {old}: {error:#}"));
                failed.push(old);
                continue;
            }
            lock.files.retain(|e| e.path != old);
            log.info(format!("Removed {old}"));
        }
        if let Err(error) = copy_into_place(&repo.cached(&entry.sha1), &repo.root.join(&entry.path)) {
            log.stream("err", &format!("Couldn't place {}: {error:#}", entry.path));
            failed.push(entry.path);
            continue;
        }
        log.info(format!("Placed {}", entry.path));
        lock.files.retain(|e| e.path != entry.path);
        lock.files.push(entry);
    }
    repo.write_lock(&mut lock, false)?;
    if !failed.is_empty() {
        let left = if failed.len() == 1 { "that mod as it was" } else { "those mods as they were" };
        bail!("packraven couldn't change {}, so it left {left}. If Minecraft is running from this pack's folder, close it and try again.", failed.join(", "));
    }
    Ok(())
}

// We copy under a name the mod list ignores, so a failed copy never leaves a partial jar for Minecraft to load.
fn copy_into_place(source: &Path, target: &Path) -> Result<()> {
    std::fs::create_dir_all(target.parent().unwrap())?;
    let partial = target.with_extension("part");
    let copied = std::fs::copy(source, &partial).map_err(anyhow::Error::from).and_then(|_| Ok(std::fs::rename(&partial, target)?));
    if copied.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    copied
}

// We keep a replaced jar in the download cache, so switching back to it needs no download.
fn retire(repo: &Repo, path: &str, sha1: &str) -> Result<()> {
    let cached = repo.cached(sha1);
    if cached.exists() {
        std::fs::remove_file(repo.root.join(path))?;
    } else {
        std::fs::rename(repo.root.join(path), cached)?;
    }
    Ok(())
}

// The caller holds MOD_LIST_GUARD. The file keeps its contents, so its lock entry only moves.
pub fn rename_locked_file(dir: &Path, from: &str, to: &str) -> Result<()> {
    let repo = Repo::open(dir)?;
    if repo.root.join(to).exists() {
        bail!("{to} already exists");
    }
    std::fs::rename(repo.root.join(from), repo.root.join(to))?;
    let mut lock = repo.read_lock()?;
    for entry in lock.files.iter_mut().filter(|e| e.path == from) {
        entry.path = to.to_string();
    }
    repo.write_lock(&mut lock, false)?;
    Ok(())
}

pub fn drop_from_lock(dir: &Path, paths: &[String]) -> Result<()> {
    let repo = Repo::open(dir)?;
    let mut lock = repo.read_lock()?;
    lock.files.retain(|e| !paths.contains(&e.path));
    repo.write_lock(&mut lock, false)?;
    Ok(())
}

pub fn missing_count(dir: &Path) -> usize {
    let Ok(text) = std::fs::read_to_string(dir.join(LOCKFILE)) else { return 0 };
    Lockfile::parse(&text).map_or(0, |lock| lock.files.iter().filter(|e| !dir.join(&e.path).exists()).count())
}

async fn sync_folder(dir: &Path, log: &Log, replace_existing: bool) -> Result<()> {
    let repo = Repo::open(dir)?;
    let lock = repo.read_lock()?;
    if let (Some(local), Some(expected)) = (read_instance(&repo.root).loader, &lock.loader)
        && local != *expected
    {
        log.stream("err", &format!("modsync: WARNING: lockfile expects {expected} but this profile uses {local}; change it in the CurseForge app"));
    }
    let mut hashes = HashCache::load(&repo);
    let mut plan = sync_plan(&repo, &lock, &mut hashes)?;
    if !replace_existing {
        plan.extra.clear();
        plan.missing.retain(|e| !repo.root.join(&e.path).exists());
    }
    if plan.missing.is_empty() && plan.extra.is_empty() {
        return hashes.save();
    }
    let _marker = RestoringMarker::create(&repo)?;
    std::fs::create_dir_all(repo.cache_dir())?;

    for (path, sha1) in &plan.extra {
        retire(&repo, path, sha1)?;
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

    fn entry(path: &str, project_id: u64, sha1: &str) -> LockEntry {
        LockEntry { path: path.into(), project_id, file_id: 0, sha1: sha1.into(), size: 0, url: String::new() }
    }

    fn lock_of(files: Vec<LockEntry>) -> Lockfile {
        Lockfile { minecraft: Some("1.21.1".into()), loader: Some("neoforge-21.1.1".into()), files }
    }

    fn paths(lock: &Lockfile) -> Vec<&str> {
        let mut paths: Vec<&str> = lock.files.iter().map(|e| e.path.as_str()).collect();
        paths.sort();
        paths
    }

    #[test]
    fn merge_keeps_changes_from_both_sides() {
        let base = lock_of(vec![entry("mods/a-1.jar", 1, "a1"), entry("mods/b.jar", 2, "b"), entry("mods/c.jar", 3, "c")]);
        let mine = lock_of(vec![entry("mods/a-2.jar", 1, "a2"), entry("mods/b.jar", 2, "b"), entry("mods/c.jar", 3, "c"), entry("mods/d.jar", 4, "d")]);
        let theirs = lock_of(vec![entry("mods/a-1.jar", 1, "a1"), entry("mods/c.jar", 3, "c"), entry("mods/e.jar", 5, "e")]);
        let merged = merge_locks(&base, &mine, &theirs, None).unwrap();
        assert_eq!(paths(&merged), ["mods/a-2.jar", "mods/c.jar", "mods/d.jar", "mods/e.jar"]);
    }

    #[test]
    fn merge_reports_mods_both_sides_changed() {
        let base = lock_of(vec![entry("mods/a-1.jar", 1, "a1")]);
        let mine = lock_of(vec![entry("mods/a-2.jar", 1, "a2")]);
        let theirs = lock_of(vec![entry("mods/a-3.jar", 1, "a3")]);
        let conflicts = merge_locks(&base, &mine, &theirs, None).unwrap_err();
        assert_eq!(conflicts, [ModConflict { name: "a-1.jar".into(), mine: Some("a-2.jar".into()), theirs: Some("a-3.jar".into()) }]);
        assert_eq!(paths(&merge_locks(&base, &mine, &theirs, Some(Prefer::Theirs)).unwrap()), ["mods/a-3.jar"]);
        assert_eq!(paths(&merge_locks(&base, &mine, &theirs, Some(Prefer::Mine)).unwrap()), ["mods/a-2.jar"]);
    }

    #[test]
    fn merge_treats_removal_against_update_as_conflict() {
        let base = lock_of(vec![entry("mods/a-1.jar", 1, "a1")]);
        let mine = lock_of(vec![]);
        let theirs = lock_of(vec![entry("mods/a-2.jar", 1, "a2")]);
        let conflicts = merge_locks(&base, &mine, &theirs, None).unwrap_err();
        assert_eq!(conflicts[0].mine, None);
        assert_eq!(conflicts[0].theirs.as_deref(), Some("a-2.jar"));
    }

    #[test]
    fn merge_matches_repeated_projects_by_path() {
        let base = lock_of(vec![entry("resourcepacks/x-dark.zip", 7, "d"), entry("resourcepacks/x-light.zip", 7, "l")]);
        let mine = lock_of(vec![entry("resourcepacks/x-dark.zip", 7, "d2"), entry("resourcepacks/x-light.zip", 7, "l")]);
        let theirs = lock_of(vec![entry("resourcepacks/x-dark.zip", 7, "d"), entry("resourcepacks/x-light.zip", 7, "l2")]);
        let merged = merge_locks(&base, &mine, &theirs, None).unwrap();
        let shas: Vec<&str> = merged.files.iter().map(|e| e.sha1.as_str()).collect();
        assert!(shas.contains(&"d2") && shas.contains(&"l2"));
    }

    #[test]
    fn counts_incoming_changes_by_project() {
        let from = lock_of(vec![entry("mods/a-1.jar", 1, "a1"), entry("mods/b.jar", 2, "b")]);
        let to = lock_of(vec![entry("mods/a-2.jar", 1, "a2"), entry("mods/c.jar", 3, "c")]);
        assert_eq!(mod_counts(&from, &to), ModCounts { added: 1, updated: 1, removed: 1 });
    }

    #[test]
    fn iris_clean_drops_timestamp_and_disables_shaders() {
        let input = "#Mon Oct 13 18:58:55 CEST 2026\r\nenableShaders=true\r\nshaderPack=x.zip\r\n";
        assert_eq!(iris_clean(input), "enableShaders=false\nshaderPack=x.zip\n");
    }

    fn test_repo(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("packraven-modsync-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("mods")).unwrap();
        git::run(&dir, &["init", "-q"]).unwrap();
        std::fs::write(dir.join(".gitignore"), "mods/*.jar\nminecraftinstance.json\n").unwrap();
        dir
    }

    #[test]
    fn refresh_moves_committed_jar_into_mod_list_once_on_curseforge() {
        let dir = test_repo("promote");
        std::fs::write(dir.join("mods/tridot.jar"), "tridot").unwrap();
        git::run(&dir, &["add", "-f", "--", "mods/tridot.jar"]).unwrap();
        git::run(&dir, &["-c", "user.name=Test", "-c", "user.email=test@example.com", "commit", "-q", "-m", "Commit tridot directly"]).unwrap();

        let sha1 = hex::encode(Sha1::digest(b"tridot"));
        let addon = serde_json::json!({ "addonID": 7, "installedFile": { "id": 70, "fileName": "tridot.jar", "downloadUrl": "u", "hashes": [{ "type": 1, "value": sha1 }] } });
        std::fs::write(dir.join(INSTANCE_JSON), serde_json::json!({ "installedAddons": [addon] }).to_string()).unwrap();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert!(runtime.block_on(refresh_mod_list(&dir)).unwrap().is_empty());
        let lock = Lockfile::parse(&std::fs::read_to_string(dir.join(LOCKFILE)).unwrap()).unwrap();
        assert_eq!(paths(&lock), ["mods/tridot.jar"]);
        assert!(!git::tracked_files(&dir).unwrap().contains(&"mods/tridot.jar".to_string()));
        assert!(dir.join("mods/tridot.jar").exists());
        assert_eq!(managed_files(&dir).unwrap(), ["mods/tridot.jar"]);
    }

    #[test]
    fn place_skips_a_mod_whose_old_jar_cant_be_removed() {
        let dir = test_repo("locked");
        let repo = Repo::open(&dir).unwrap();
        std::fs::create_dir_all(repo.cache_dir()).unwrap();
        // remove_file fails on a directory, the way it fails on a jar Minecraft holds open on Windows.
        std::fs::create_dir_all(dir.join("mods/old.jar")).unwrap();
        for (name, content) in [("new.jar", "new"), ("other.jar", "other")] {
            let sha1 = hex::encode(Sha1::digest(content));
            std::fs::write(repo.cached(&sha1), content).unwrap();
            let replaces = (name == "new.jar").then(|| "mods/old.jar".to_string());
            let placement = Placement { entry: LockEntry { path: format!("mods/{name}"), project_id: 1, file_id: 1, sha1, size: 3, url: String::new() }, replaces };
            let result = tauri::async_runtime::block_on(place(&dir, vec![placement], &Log::console("test")));
            assert_eq!(result.is_ok(), name == "other.jar", "{name}: {result:?}");
        }
        assert!(!dir.join("mods/new.jar").exists(), "we must not place a second version next to the old one");
        assert!(dir.join("mods/old.jar").exists());
        assert_eq!(paths(&repo.read_lock().unwrap()), ["mods/other.jar"]);
    }

    #[test]
    fn not_on_curseforge_forgets_old_misses() {
        let dir = test_repo("misses");
        let repo = Repo::open(&dir).unwrap();
        let stale = unix_now() - NOT_ON_CURSEFORGE_RECHECK.as_secs() - 1;
        std::fs::create_dir_all(&repo.state_dir).unwrap();
        std::fs::write(repo.state_dir.join("not-on-curseforge.json"), serde_json::json!({ "old": stale, "new": unix_now() }).to_string()).unwrap();

        let misses = NotOnCurseforge::load(&repo);
        assert!(!misses.checked_within("old", NOT_ON_CURSEFORGE_RECHECK));
        assert!(misses.checked_within("new", NOT_ON_CURSEFORGE_RECHECK));
    }

    #[test]
    fn uncommitted_misses_expire_within_approval_time() {
        let dir = test_repo("uncommitted-misses");
        let repo = Repo::open(&dir).unwrap();
        let two_minutes_ago = unix_now() - 120;
        std::fs::create_dir_all(&repo.state_dir).unwrap();
        std::fs::write(repo.state_dir.join("not-on-curseforge.json"), serde_json::json!({ "uploaded": two_minutes_ago, "new": unix_now() }).to_string()).unwrap();

        let misses = NotOnCurseforge::load(&repo);
        assert!(!misses.checked_within("uploaded", UNCOMMITTED_RECHECK));
        assert!(misses.checked_within("uploaded", NOT_ON_CURSEFORGE_RECHECK));
        assert!(misses.checked_within("new", UNCOMMITTED_RECHECK));
    }
}
