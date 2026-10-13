use std::collections::HashSet;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

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
    let mut command = Command::new("git");
    command.current_dir(root).args(["check-ignore", "-z", "--stdin"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    crate::process::hide_window_std(&mut command);
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
    let mut entries: Vec<Entry> = std::fs::read_dir(&dir)
        .with_context(|| format!("reading {}", dir.display()))?
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

#[cfg(test)]
mod tests {
    use super::*;

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
