use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use anyhow::{Context, Result, bail};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::history::Rewrite;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    pub pattern: String,
    pub regex: bool,
    pub match_case: bool,
    pub whole_word: bool,
    pub include: String,
    pub exclude: String,
    pub include_ignored: bool,
    /// When set, each match also carries the text that replacing it would insert.
    pub replace: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Results {
    pub files: Vec<FileMatches>,
    pub total: usize,
    pub truncated: bool,
    pub searched: usize,
    pub too_large: usize,
}

#[derive(Serialize)]
pub struct FileMatches {
    pub path: String,
    pub matches: Vec<Match>,
}

/// A match on one line. `column` and `length` count UTF-16 units, as the editor does, and
/// `start` and `end` mark the match inside `preview`.
#[derive(Serialize)]
pub struct Match {
    pub line: u32,
    pub column: u32,
    pub length: u32,
    pub preview: String,
    pub start: u32,
    pub end: u32,
    pub replacement: Option<String>,
}

#[derive(Deserialize)]
pub struct Target {
    pub path: String,
    /// The (line, column) of each match to replace, as the search reported them.
    pub matches: Vec<(u32, u32)>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Replaced {
    pub label: String,
    pub files: Vec<String>,
    pub replaced: usize,
    pub missed: usize,
    pub undoable: bool,
    #[serde(skip)]
    pub rewrites: Vec<Rewrite>,
}

const MAX_MATCHES: usize = 10_000;
const PREVIEW_BEFORE: usize = 30;
const PREVIEW_CHARS: usize = 250;
const BINARY_SNIFF: usize = 8000;
// Undo keeps each file's old and new text in memory, so we skip it for a replace this large.
const MAX_UNDO_BYTES: usize = 64 * 1024 * 1024;

// You type faster than a big pack searches, so each search makes the ones still running stop.
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn compile(query: &Query) -> Result<Regex> {
    let body = if query.regex { query.pattern.clone() } else { regex::escape(&query.pattern) };
    let body = if query.whole_word { format!(r"\b(?:{body})\b") } else { body };
    RegexBuilder::new(&body).case_insensitive(!query.match_case).build().map_err(|e| match e {
        regex::Error::Syntax(text) => anyhow::anyhow!("That isn't a valid regex: {}.", text.lines().last().unwrap_or_default().trim_start_matches("error: ")),
        other => anyhow::anyhow!("That regex is too large to search with. {other}"),
    })
}

// A pattern without a slash matches a file or folder name anywhere, so `config` and `*.toml`
// work as you'd expect. A slash anchors it to the pack folder.
fn globs(list: &str) -> Result<Option<GlobSet>> {
    let mut builder = GlobSetBuilder::new();
    let mut any = false;
    for pattern in list.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let trimmed = pattern.trim_start_matches("./").trim_matches('/');
        if trimmed.is_empty() {
            continue;
        }
        let base = if pattern.starts_with('/') || trimmed.contains('/') { trimmed.to_string() } else { format!("**/{trimmed}") };
        for glob in [base.clone(), format!("{base}/**")] {
            let built = GlobBuilder::new(&glob).literal_separator(true).case_insensitive(cfg!(any(windows, target_os = "macos"))).build();
            builder.add(built.with_context(|| format!("{pattern} isn't a valid file pattern"))?);
        }
        any = true;
    }
    Ok(if any { Some(builder.build()?) } else { None })
}

// Git already knows which files your .gitignore leaves out, including nested ignore files and
// negations, so we ask it rather than reimplement the rules.
fn candidates(root: &Path, include_ignored: bool) -> Vec<String> {
    if !include_ignored {
        let output = crate::git::command().current_dir(root).args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"]).output();
        if let Some(output) = output.ok().filter(|o| o.status.success()) {
            let mut paths: Vec<String> = String::from_utf8_lossy(&output.stdout).split('\0').filter(|p| !p.is_empty()).map(str::to_string).collect();
            // A file with a merge conflict appears once for each side.
            paths.dedup();
            return paths;
        }
    }
    walkdir::WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git")
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| Some(e.path().strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/")))
        .collect()
}

enum Text {
    Searchable { bom: bool, text: String },
    TooLarge,
    Skipped,
}

fn read_text(path: &Path, limit: u64) -> Text {
    let Ok(metadata) = std::fs::metadata(path) else { return Text::Skipped };
    if !metadata.is_file() {
        return Text::Skipped;
    }
    if metadata.len() > limit {
        return Text::TooLarge;
    }
    let Ok(bytes) = std::fs::read(path) else { return Text::Skipped };
    if bytes.iter().take(BINARY_SNIFF).any(|b| *b == 0) {
        return Text::Skipped;
    }
    let Ok(text) = String::from_utf8(bytes) else { return Text::Skipped };
    match text.strip_prefix('\u{feff}') {
        Some(body) => Text::Searchable { bom: true, text: body.to_string() },
        None => Text::Searchable { bom: false, text },
    }
}

fn utf16_len(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

fn back_chars(text: &str, from: usize, count: usize) -> usize {
    text[..from].char_indices().rev().nth(count.saturating_sub(1)).map_or(0, |(at, _)| at)
}

fn forward_chars(text: &str, from: usize, count: usize) -> usize {
    text[from..].char_indices().nth(count).map_or(text.len(), |(at, _)| from + at)
}

// We drop the line's indentation and keep a little text before the match, so the match stays
// in view in a narrow sidebar.
fn preview(line: &str, start: usize, end: usize) -> (String, u32, u32) {
    let indent = line.len() - line.trim_start().len();
    let mut from = indent.min(start);
    let mut text = String::new();
    if line[from..start].chars().count() > PREVIEW_BEFORE {
        from = back_chars(line, start, PREVIEW_BEFORE);
        text.push('…');
    }
    let to = forward_chars(line, from, PREVIEW_CHARS);
    let shown_end = end.min(to);
    let match_start = utf16_len(&text) + utf16_len(&line[from..start]);
    let match_end = match_start + utf16_len(&line[start..shown_end]);
    text.push_str(&line[from..to]);
    if to < line.len() {
        text.push('…');
    }
    (text, match_start, match_end)
}

// Rust's regex crate reads `$1a` as a group named "1a", while JavaScript and VS Code read it as
// group 1 followed by "a", so we brace numbered groups to match what you'd expect.
fn brace_groups(replacement: &str) -> String {
    let mut out = String::with_capacity(replacement.len());
    let mut chars = replacement.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('$') => {
                chars.next();
                out.push_str("$$");
            }
            Some(d) if d.is_ascii_digit() => {
                out.push_str("${");
                while let Some(d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                    out.push(*d);
                    chars.next();
                }
                out.push('}');
            }
            _ => out.push('$'),
        }
    }
    out
}

struct Replacer {
    template: String,
    expand: bool,
}

impl Replacer {
    fn new(query: &Query, replacement: &str) -> Self {
        if query.regex { Replacer { template: brace_groups(replacement), expand: true } } else { Replacer { template: replacement.to_string(), expand: false } }
    }

    fn apply(&self, captures: &regex::Captures) -> String {
        if !self.expand {
            return self.template.clone();
        }
        let mut out = String::new();
        captures.expand(&self.template, &mut out);
        out
    }
}

struct LineMatch<'a> {
    line: u32,
    /// Byte offsets into the whole text.
    start: usize,
    end: usize,
    line_text: &'a str,
    line_start: usize,
    captures: regex::Captures<'a>,
}

impl LineMatch<'_> {
    fn column(&self) -> u32 {
        utf16_len(&self.line_text[..self.start - self.line_start])
    }
}

// We match line by line, so ^ and $ mean the start and end of a line, and a CRLF file's \r
// never counts as part of the line.
fn line_matches<'a>(text: &'a str, regex: &'a Regex) -> impl Iterator<Item = LineMatch<'a>> + 'a {
    let mut line_start = 0;
    text.split('\n').enumerate().flat_map(move |(index, raw)| {
        let start = line_start;
        line_start += raw.len() + 1;
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        regex.captures_iter(line).filter_map(move |captures| {
            let found = captures.get(0)?;
            (!found.is_empty()).then(|| LineMatch { line: index as u32 + 1, start: start + found.start(), end: start + found.end(), line_text: line, line_start: start, captures })
        })
    })
}

pub fn search(root: &Path, query: &Query, limit: u64) -> Result<Option<Results>> {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    if query.pattern.is_empty() {
        return Ok(Some(Results { files: Vec::new(), total: 0, truncated: false, searched: 0, too_large: 0 }));
    }
    let regex = compile(query)?;
    let include = globs(&query.include)?;
    let exclude = globs(&query.exclude)?;
    let replacer = query.replace.as_deref().map(|r| Replacer::new(query, r));
    let paths: Vec<String> = candidates(root, query.include_ignored)
        .into_iter()
        .filter(|p| include.as_ref().is_none_or(|set| set.is_match(p)) && !exclude.as_ref().is_some_and(|set| set.is_match(p)))
        .collect();

    let next = AtomicUsize::new(0);
    let total = AtomicUsize::new(0);
    let searched = AtomicUsize::new(0);
    let too_large = AtomicUsize::new(0);
    let found = Mutex::new(Vec::new());
    let superseded = || GENERATION.load(Ordering::SeqCst) != generation;
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(8);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= paths.len() || total.load(Ordering::Relaxed) >= MAX_MATCHES || superseded() {
                        break;
                    }
                    let path = &paths[index];
                    let text = match read_text(&root.join(path), limit) {
                        Text::Searchable { text, .. } => text,
                        Text::TooLarge => {
                            too_large.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        Text::Skipped => continue,
                    };
                    searched.fetch_add(1, Ordering::Relaxed);
                    let matches: Vec<Match> = line_matches(&text, &regex)
                        .take(MAX_MATCHES)
                        .map(|m| {
                            let (preview, start, end) = preview(m.line_text, m.start - m.line_start, m.end - m.line_start);
                            Match { line: m.line, column: m.column(), length: utf16_len(&text[m.start..m.end]), preview, start, end, replacement: replacer.as_ref().map(|r| r.apply(&m.captures)) }
                        })
                        .collect();
                    if !matches.is_empty() {
                        total.fetch_add(matches.len(), Ordering::Relaxed);
                        found.lock().unwrap().push(FileMatches { path: path.clone(), matches });
                    }
                }
            });
        }
    });
    if superseded() {
        return Ok(None);
    }

    let mut files = found.into_inner().unwrap();
    files.sort_by_cached_key(|f| f.path.to_lowercase());
    let mut kept = 0;
    let mut truncated = false;
    files.retain_mut(|file| {
        if kept >= MAX_MATCHES {
            truncated = true;
            return false;
        }
        if kept + file.matches.len() > MAX_MATCHES {
            file.matches.truncate(MAX_MATCHES - kept);
            truncated = true;
        }
        kept += file.matches.len();
        true
    });
    Ok(Some(Results { files, total: kept, truncated, searched: searched.into_inner(), too_large: too_large.into_inner() }))
}

// We search again rather than trust the positions you saw, since the file may have changed
// since. A match that's no longer where the search found it counts as missed.
pub fn replace(root: &Path, query: &Query, targets: &[Target], limit: u64) -> Result<Replaced> {
    let Some(replacement) = query.replace.as_deref() else { bail!("Type the text to replace matches with.") };
    if query.pattern.is_empty() {
        bail!("Type something to search for first.");
    }
    let regex = compile(query)?;
    let replacer = Replacer::new(query, replacement);
    let mut planned: Vec<(PathBuf, String, Vec<u8>, Vec<u8>)> = Vec::new();
    let mut replaced = 0;
    let mut missed = 0;
    for target in targets {
        let path = crate::files::resolve_entry(root, &target.path)?;
        let wanted: HashSet<(u32, u32)> = target.matches.iter().copied().collect();
        let (bom, text) = match read_text(&path, limit) {
            Text::Searchable { bom, text } => (bom, text),
            _ => {
                missed += wanted.len();
                continue;
            }
        };
        let mut out = String::with_capacity(text.len());
        let mut copied = 0;
        let mut hits = 0;
        for found in line_matches(&text, &regex) {
            if !wanted.contains(&(found.line, found.column())) {
                continue;
            }
            out.push_str(&text[copied..found.start]);
            out.push_str(&replacer.apply(&found.captures));
            copied = found.end;
            hits += 1;
        }
        missed += wanted.len() - hits;
        if hits == 0 {
            continue;
        }
        out.push_str(&text[copied..]);
        let prefix = if bom { "\u{feff}" } else { "" };
        let before = format!("{prefix}{text}").into_bytes();
        let after = format!("{prefix}{out}").into_bytes();
        replaced += hits;
        planned.push((path, target.path.clone(), before, after));
    }

    let mut files = Vec::new();
    let mut rewrites = Vec::new();
    for (path, relative, before, after) in planned {
        crate::files::replace_contents(&path, &after).with_context(|| format!("writing {relative}"))?;
        files.push(relative);
        rewrites.push(Rewrite { path, expected: after, restore: before });
    }
    let undoable = rewrites.iter().map(|r| r.expected.len() + r.restore.len()).sum::<usize>() <= MAX_UNDO_BYTES;
    if !undoable {
        rewrites.clear();
    }
    let label = format!("Replace in {}", crate::history::describe(&files));
    Ok(Replaced { label, files, replaced, missed, undoable, rewrites })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(pattern: &str) -> Query {
        Query { pattern: pattern.into(), regex: false, match_case: false, whole_word: false, include: String::new(), exclude: String::new(), include_ignored: true, replace: None }
    }

    // Each search cancels the ones still running, so tests that search can't run side by side.
    static SEARCHING: Mutex<()> = Mutex::new(());

    fn pack(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("config/sub")).unwrap();
        dir
    }

    #[test]
    fn finds_matches_with_utf16_columns_and_filters() {
        let _alone = SEARCHING.lock().unwrap();
        let dir = pack("packraven-text-search");
        std::fs::write(dir.join("config/a.toml"), "\u{feff}name = \"Ünïcode 🐦 raven\"\r\nother = 1\n    raven = true\n").unwrap();
        std::fs::write(dir.join("config/sub/b.json"), "{\"raven\": 2}").unwrap();
        std::fs::write(dir.join("binary.dat"), b"raven\0\0").unwrap();

        let results = search(&dir, &query("RAVEN"), 1_000_000).unwrap().unwrap();
        assert_eq!(results.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["config/a.toml", "config/sub/b.json"]);
        let first = &results.files[0].matches;
        assert_eq!((first[0].line, first[0].column, first[0].length), (1, 19, 5));
        assert_eq!((first[1].line, first[1].column), (3, 4));
        assert_eq!(&first[1].preview[first[1].start as usize..first[1].end as usize], "raven");

        let toml_only = search(&dir, &Query { include: "*.toml".into(), ..query("raven") }, 1_000_000).unwrap().unwrap();
        assert_eq!(toml_only.files.len(), 1);
        let no_sub = search(&dir, &Query { exclude: "sub".into(), ..query("raven") }, 1_000_000).unwrap().unwrap();
        assert_eq!(no_sub.files.len(), 1);
        let anchored = search(&dir, &Query { regex: true, ..query("^raven") }, 1_000_000).unwrap().unwrap();
        assert_eq!(anchored.total, 0);
        let whole = search(&dir, &Query { whole_word: true, ..query("rave") }, 1_000_000).unwrap().unwrap();
        assert_eq!(whole.total, 0);
        assert!(search(&dir, &Query { regex: true, ..query("(") }, 1_000_000).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn replaces_only_the_chosen_matches_and_keeps_line_endings() {
        let _alone = SEARCHING.lock().unwrap();
        let dir = pack("packraven-text-replace");
        std::fs::write(dir.join("config/a.toml"), "\u{feff}a = 1\r\nb = 1\r\nc = 1\r\n").unwrap();
        let replace = Query { regex: true, replace: Some("$1x2".into()), ..query(r"(\w) = 1") };
        let found = search(&dir, &replace, 1_000_000).unwrap().unwrap();
        assert_eq!(found.files[0].matches[0].replacement.as_deref(), Some("ax2"));

        let targets = [Target { path: "config/a.toml".into(), matches: vec![(1, 0), (3, 0), (9, 0)] }];
        let done = super::replace(&dir, &replace, &targets, 1_000_000).unwrap();
        assert_eq!((done.replaced, done.missed), (2, 1));
        assert_eq!(std::fs::read_to_string(dir.join("config/a.toml")).unwrap(), "\u{feff}ax2\r\nb = 1\r\ncx2\r\n");
        assert_eq!(done.rewrites[0].restore, "\u{feff}a = 1\r\nb = 1\r\nc = 1\r\n".as_bytes());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn previews_keep_the_match_in_view() {
        let line = format!("{}needle{}", "x".repeat(100), "y".repeat(400));
        let (text, start, end) = preview(&line, 100, 106);
        assert!(text.starts_with('…') && text.ends_with('…'));
        assert_eq!(&text[text.char_indices().nth(start as usize).unwrap().0..][..6], "needle");
        assert_eq!(end - start, 6);
    }

    #[test]
    fn numbered_groups_are_braced() {
        assert_eq!(brace_groups("$1a $$1 $name ${2}"), "${1}a $$1 $name ${2}");
    }
}
