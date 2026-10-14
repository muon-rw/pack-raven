use std::collections::HashSet;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;

use anyhow::{bail, Context, Result};
use serde::Serialize;

const MAX_EDITABLE_BYTES: u64 = 2_000_000;
const HIDDEN: [&str; 1] = [".git"];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub ignored: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileContent {
    pub text: Option<String>,
    pub size: u64,
    pub unreadable: Option<String>,
    pub crlf: bool,
    pub bom: bool,
}

pub fn resolve(root: &Path, relative: &str) -> Result<PathBuf> {
    let relative = Path::new(relative);
    if relative.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        bail!("{} is outside the pack folder", relative.display());
    }
    Ok(root.join(relative))
}

fn ignored(root: &Path, paths: &[String]) -> HashSet<String> {
    if paths.is_empty() {
        return HashSet::new();
    }
    let mut command = crate::git::command();
    command.current_dir(root).args(["check-ignore", "-z", "--stdin"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    let Ok(mut child) = command.spawn() else { return HashSet::new() };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(paths.join("\0").as_bytes());
        let _ = stdin.write_all(b"\0");
    }
    let Ok(output) = child.wait_with_output() else { return HashSet::new() };
    String::from_utf8_lossy(&output.stdout).split('\0').filter(|p| !p.is_empty()).map(|p| p.trim_end_matches('/').to_string()).collect()
}

pub fn list(root: &Path, relative: &str) -> Result<Vec<Entry>> {
    let dir = resolve(root, relative)?;
    let prefix = if relative.is_empty() { String::new() } else { format!("{}/", relative.trim_end_matches('/')) };
    // A folder you just deleted or moved can still be expanded in the tree for a moment.
    let listing = match std::fs::read_dir(&dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !relative.is_empty() => return Ok(Vec::new()),
        listing => listing.with_context(|| format!("reading {}", dir.display()))?,
    };
    let mut entries: Vec<Entry> = listing
        .flatten()
        .filter(|e| !HIDDEN.contains(&e.file_name().to_string_lossy().as_ref()))
        .filter_map(|e| {
            let metadata = e.metadata().ok()?;
            let name = e.file_name().to_string_lossy().into_owned();
            Some(Entry { path: format!("{prefix}{name}"), name, is_dir: metadata.is_dir(), size: metadata.len(), ignored: false })
        })
        .collect();
    let queries: Vec<String> = entries.iter().map(|e| if e.is_dir { format!("{}/", e.path) } else { e.path.clone() }).collect();
    let ignored = ignored(root, &queries);
    for entry in &mut entries {
        entry.ignored = ignored.contains(&entry.path);
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(entries)
}

pub fn read(root: &Path, relative: &str) -> Result<FileContent> {
    let path = resolve(root, relative)?;
    let size = std::fs::metadata(&path)?.len();
    let unreadable = |reason: &str| FileContent { text: None, size, unreadable: Some(reason.to_string()), crlf: false, bom: false };
    if size > MAX_EDITABLE_BYTES {
        return Ok(unreadable("This file is too large to edit here."));
    }
    let bytes = std::fs::read(&path)?;
    if bytes.iter().take(8000).any(|b| *b == 0) {
        return Ok(unreadable("This is a binary file, so you can't edit it as text."));
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return Ok(unreadable("This file isn't UTF-8 text, so you can't edit it here."));
    };
    let bom = text.starts_with('\u{feff}');
    let body = text.trim_start_matches('\u{feff}');
    let crlf = body.contains("\r\n");
    Ok(FileContent { text: Some(body.replace("\r\n", "\n")), size, unreadable: None, crlf, bom })
}

pub fn write(root: &Path, relative: &str, text: &str, crlf: bool, bom: bool) -> Result<()> {
    let path = resolve(root, relative)?;
    let mut body = if crlf { text.replace("\r\n", "\n").replace('\n', "\r\n") } else { text.to_string() };
    if bom {
        body.insert(0, '\u{feff}');
    }
    let temp = path.with_extension("packraven-tmp");
    std::fs::write(&temp, body).with_context(|| format!("writing {}", path.display()))?;
    std::fs::rename(&temp, &path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

fn resolve_entry(root: &Path, relative: &str) -> Result<PathBuf> {
    if relative.trim_matches('/').is_empty() {
        bail!("pick a file or folder inside the pack folder");
    }
    resolve(root, relative)
}

pub fn create_file(root: &Path, relative: &str) -> Result<()> {
    let path = resolve_entry(root, relative)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::OpenOptions::new().write(true).create_new(true).open(&path).with_context(|| format!("creating {relative}"))?;
    Ok(())
}

pub fn create_dir(root: &Path, relative: &str) -> Result<()> {
    let path = resolve_entry(root, relative)?;
    if path.exists() {
        bail!("{relative} already exists");
    }
    std::fs::create_dir_all(&path).with_context(|| format!("creating {relative}"))
}

pub fn trash(root: &Path, relatives: &[String]) -> Result<Option<Vec<trash::TrashItem>>> {
    let paths = relatives.iter().map(|r| resolve_entry(root, r)).collect::<Result<Vec<_>>>()?;
    crate::history::trash(&paths)
}

pub fn absolute_paths(root: &Path, relatives: &[String]) -> Result<Vec<String>> {
    relatives.iter().map(|r| Ok(resolve_entry(root, r)?.to_string_lossy().into_owned())).collect()
}

// Like Explorer and Finder, we never overwrite on paste. We name a clashing copy
// "name copy.ext", then "name copy 2.ext", and so on.
fn free_name(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(index) if index > 0 => name.split_at(index),
        _ => (name, ""),
    };
    (1..)
        .map(|n| dir.join(if n == 1 { format!("{stem} copy{ext}") } else { format!("{stem} copy {n}{ext}") }))
        .find(|p| !p.exists())
        .expect("an unused name")
}

fn copy_tree(source: &Path, target: &Path) -> Result<()> {
    if !source.is_dir() {
        std::fs::copy(source, target).with_context(|| format!("copying {}", source.display()))?;
        return Ok(());
    }
    for entry in walkdir::WalkDir::new(source) {
        let entry = entry?;
        let dest = target.join(entry.path().strip_prefix(source)?);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&dest).with_context(|| format!("creating {}", dest.display()))?;
        } else {
            std::fs::copy(entry.path(), &dest).with_context(|| format!("copying {}", entry.path().display()))?;
        }
    }
    Ok(())
}

fn same_entry(a: &Path, b: &Path) -> bool {
    matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

pub fn rename(root: &Path, from: &str, to: &str) -> Result<()> {
    let source = resolve_entry(root, from)?;
    let target = resolve_entry(root, to)?;
    // On Windows and macOS, a case-only rename finds the source itself at the target path.
    if target.exists() && !same_entry(&source, &target) {
        bail!("{to} already exists");
    }
    std::fs::rename(&source, &target).with_context(|| format!("renaming {from} to {to}"))
}

/// Moves each entry into `relative_dir` and returns the (old, new) path of every entry that moved.
pub fn move_into(root: &Path, relatives: &[String], relative_dir: &str) -> Result<Vec<(String, String)>> {
    let dir = resolve(root, relative_dir)?;
    let prefix = if relative_dir.is_empty() { String::new() } else { format!("{}/", relative_dir.trim_end_matches('/')) };
    let mut moves = Vec::new();
    // We check every entry before moving any, so a clash can't leave the selection half moved.
    for relative in relatives {
        let source = resolve_entry(root, relative)?;
        let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if dir.starts_with(&source) {
            bail!("you can't move {name} into itself");
        }
        let target = dir.join(&name);
        if source.parent() == Some(dir.as_path()) {
            continue;
        }
        if target.exists() {
            bail!("{} already has a {name}", if relative_dir.is_empty() { "The pack folder" } else { relative_dir });
        }
        moves.push((relative.clone(), format!("{prefix}{name}"), source, target));
    }
    for (from, to, source, target) in &moves {
        std::fs::rename(source, target).with_context(|| format!("moving {from} to {to}"))?;
    }
    Ok(moves.into_iter().map(|(from, to, ..)| (from, to)).collect())
}

#[derive(Serialize, Default, Debug)]
pub struct Pasted {
    pub created: Vec<String>,
    /// The (old, new) paths of moved entries that were already inside the pack folder.
    pub moved: Vec<(String, String)>,
    /// The (old, new) absolute paths of every moved entry, including ones from outside the pack.
    #[serde(skip)]
    pub transfers: Vec<(PathBuf, PathBuf)>,
    #[serde(skip)]
    pub copies: Vec<PathBuf>,
}

pub(crate) fn move_entry(source: &Path, target: &Path) -> Result<()> {
    match std::fs::rename(source, target) {
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            copy_tree(source, target)?;
            if source.is_dir() { std::fs::remove_dir_all(source) } else { std::fs::remove_file(source) }
                .with_context(|| format!("removing {} after copying it", source.display()))
        }
        result => result.with_context(|| format!("moving {}", source.display())),
    }
}

pub fn paste(root: &Path, relative_dir: &str, sources: &[PathBuf], cut: bool) -> Result<Pasted> {
    let dir = resolve(root, relative_dir)?;
    let canonical_dir = dir.canonicalize().with_context(|| format!("reading {}", dir.display()))?;
    let canonical_root = root.canonicalize().with_context(|| format!("reading {}", root.display()))?;
    let prefix = if relative_dir.is_empty() { String::new() } else { format!("{}/", relative_dir.trim_end_matches('/')) };
    let mut pasted = Pasted::default();
    for source in sources {
        let Some(name) = source.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
        let canonical_source = source.canonicalize().with_context(|| format!("reading {}", source.display()))?;
        if canonical_source.is_dir() && canonical_dir.starts_with(&canonical_source) {
            bail!("you can't paste {name} into itself");
        }
        // Moving an entry into the folder it's already in leaves it where it is, as in Explorer.
        if cut && canonical_source.parent() == Some(canonical_dir.as_path()) {
            continue;
        }
        let target = free_name(&dir, &name);
        let to = format!("{prefix}{}", target.file_name().unwrap_or_default().to_string_lossy());
        if cut {
            move_entry(source, &target)?;
            if let Ok(inside) = canonical_source.strip_prefix(&canonical_root) {
                pasted.moved.push((inside.to_string_lossy().replace('\\', "/"), to.clone()));
            }
            pasted.transfers.push((source.clone(), target));
        } else {
            copy_tree(source, &target)?;
            pasted.copies.push(target);
        }
        pasted.created.push(to);
    }
    Ok(pasted)
}

#[cfg(windows)]
fn same_volume(a: &Path, b: &Path) -> bool {
    let volume = |p: &Path| match p.canonicalize().ok()?.components().next()? {
        Component::Prefix(prefix) => Some(prefix.as_os_str().to_ascii_lowercase()),
        _ => None,
    };
    volume(a).is_some_and(|v| Some(v) == volume(b))
}

#[cfg(unix)]
fn same_volume(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    matches!((a.metadata(), b.metadata()), (Ok(a), Ok(b)) if a.dev() == b.dev())
}

/// Brings files you drop on the window into the pack. Like Explorer and Finder, we move files
/// from the same drive and copy them from another, unless `requested_move` says otherwise.
pub fn import(root: &Path, relative_dir: &str, sources: &[PathBuf], requested_move: Option<bool>) -> Result<Pasted> {
    let cut = requested_move.unwrap_or_else(|| sources.iter().all(|s| same_volume(s, root)));
    paste(root, relative_dir, sources, cut)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_never_overwrites() {
        let dir = std::env::temp_dir().join("packraven-paste-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("config/nested")).unwrap();
        std::fs::write(dir.join("config/a.json"), "{}").unwrap();
        std::fs::write(dir.join("config/nested/b.toml"), "").unwrap();
        let sources = [dir.join("config/a.json"), dir.join("config/nested")];
        assert_eq!(paste(&dir, "config", &sources, false).unwrap().created, ["config/a copy.json", "config/nested copy"]);
        assert_eq!(paste(&dir, "config", &sources[..1], false).unwrap().created, ["config/a copy 2.json"]);
        assert!(dir.join("config/nested copy/b.toml").exists());
        assert!(paste(&dir, "config/nested", &sources[1..], false).is_err());
    }

    #[test]
    fn cut_paste_moves_from_inside_and_outside_the_pack() {
        let dir = std::env::temp_dir().join("packraven-cut-test");
        let outside = std::env::temp_dir().join("packraven-cut-test-outside");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::create_dir_all(dir.join("kubejs")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(dir.join("config/a.json"), "").unwrap();
        std::fs::write(outside.join("b.toml"), "").unwrap();
        let pasted = paste(&dir, "kubejs", &[dir.join("config/a.json"), outside.join("b.toml")], true).unwrap();
        assert_eq!(pasted.created, ["kubejs/a.json", "kubejs/b.toml"]);
        assert_eq!(pasted.moved, [("config/a.json".to_string(), "kubejs/a.json".to_string())]);
        assert!(!dir.join("config/a.json").exists() && !outside.join("b.toml").exists());
        let unchanged = paste(&dir, "kubejs", &[dir.join("kubejs/a.json")], true).unwrap();
        assert!(unchanged.created.is_empty() && dir.join("kubejs/a.json").exists());
    }

    #[test]
    fn import_moves_on_the_same_drive_unless_you_ask_to_copy() {
        let dir = std::env::temp_dir().join("packraven-import-test");
        let outside = std::env::temp_dir().join("packraven-import-test-outside");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(dir.join("a.json"), "").unwrap();
        std::fs::write(outside.join("b.toml"), "").unwrap();
        assert_eq!(import(&dir, "config", &[outside.join("b.toml")], Some(false)).unwrap().created, ["config/b.toml"]);
        assert!(outside.join("b.toml").exists(), "a requested copy keeps the original");
        assert_eq!(import(&dir, "config", &[dir.join("a.json")], None).unwrap().moved, [("a.json".to_string(), "config/a.json".to_string())]);
        assert!(!dir.join("a.json").exists(), "the same drive moves by default");
    }

    #[test]
    fn moves_and_renames_without_clobbering() {
        let dir = std::env::temp_dir().join("packraven-move-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("config/nested")).unwrap();
        std::fs::create_dir_all(dir.join("kubejs")).unwrap();
        std::fs::write(dir.join("config/a.json"), "").unwrap();
        std::fs::write(dir.join("kubejs/a.json"), "").unwrap();
        assert!(move_into(&dir, &["config".into()], "config/nested").is_err());
        assert!(move_into(&dir, &["config/a.json".into(), "config/nested".into()], "kubejs").is_err());
        assert!(dir.join("config/nested").exists(), "a clash moves nothing");
        let moved = move_into(&dir, &["config/nested".into(), "kubejs/a.json".into()], "kubejs").unwrap();
        assert_eq!(moved, [("config/nested".to_string(), "kubejs/nested".to_string())]);
        assert!(rename(&dir, "config/a.json", "kubejs/a.json").is_err());
        rename(&dir, "config/a.json", "config/A.json").unwrap();
        assert!(dir.join("config").read_dir().unwrap().any(|e| e.unwrap().file_name() == "A.json"));
    }

    #[test]
    fn refuses_to_touch_the_pack_root() {
        let root = Path::new("C:/pack");
        assert!(resolve_entry(root, "").is_err());
        assert!(resolve_entry(root, "/").is_err());
        assert!(trash(root, &[String::new()]).is_err());
    }

    #[test]
    fn resolve_rejects_escapes() {
        let root = Path::new("C:/pack");
        assert!(resolve(root, "config/a.toml").is_ok());
        assert!(resolve(root, "../outside.txt").is_err());
        assert!(resolve(root, "config/../../x").is_err());
        assert!(resolve(root, "C:/Windows/win.ini").is_err());
        assert!(resolve(root, "/etc/passwd").is_err());
    }

    #[test]
    fn round_trips_line_endings_and_bom() {
        let dir = std::env::temp_dir().join("packraven-files-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.toml"), "\u{feff}a = 1\r\nb = 2\r\n").unwrap();
        let content = read(&dir, "a.toml").unwrap();
        assert!(content.crlf && content.bom);
        assert_eq!(content.text.as_deref(), Some("a = 1\nb = 2\n"));
        write(&dir, "a.toml", "a = 3\nb = 2\n", content.crlf, content.bom).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.toml")).unwrap(), "\u{feff}a = 3\r\nb = 2\r\n");
    }
}
