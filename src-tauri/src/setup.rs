use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::process::Command;

use crate::git;
use crate::jobs::Log;
use crate::pack::{PackConfig, CONFIG_FILE, LOCKFILE};

const HOOKS: [(&str, &str); 7] = [
    ("modsync.py", include_str!("../resources/githooks/modsync.py")),
    ("iris_clean.py", include_str!("../resources/githooks/iris_clean.py")),
    ("run.sh", include_str!("../resources/githooks/run.sh")),
    ("pre-commit", include_str!("../resources/githooks/pre-commit")),
    ("post-merge", include_str!("../resources/githooks/post-merge")),
    ("post-checkout", include_str!("../resources/githooks/post-checkout")),
    ("post-rewrite", include_str!("../resources/githooks/post-rewrite")),
];
const GITIGNORE_TEMPLATE: &str = include_str!("../resources/pack.gitignore");
const GITATTRIBUTES: [&str; 3] = [".githooks/* text eol=lf", "mods.lock.json text eol=lf", "config/iris.properties filter=iris"];
const BINARY_PATTERNS: [&str; 6] = ["mods/*.jar", "mods/*.disabled", "resourcepacks/*.zip", "shaderpacks/*.zip", "datapacks/*.zip", "config/spark"];

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HooksState {
    Missing,
    Outdated,
    Current,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceInfo {
    pub dir: PathBuf,
    pub name: String,
    pub minecraft: Option<String>,
    pub loader: Option<String>,
    pub addon_count: usize,
    pub has_git: bool,
    pub has_lock: bool,
    pub hooks: HooksState,
    pub tracked_binaries: Vec<TrackedBinary>,
    pub remote: Option<String>,
    pub added: bool,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BinaryKind {
    Curseforge,
    Local,
    Generated,
}

#[derive(Serialize)]
pub struct TrackedBinary {
    pub path: String,
    pub kind: BinaryKind,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupOptions {
    pub remote: Option<String>,
    pub author: String,
    pub project_id: u64,
    pub untrack_binaries: bool,
    pub commit: Option<FirstCommit>,
}

#[derive(Deserialize)]
pub struct FirstCommit {
    pub message: String,
    pub author: Option<String>,
}

fn hooks_state(dir: &Path) -> HooksState {
    let hooks = dir.join(".githooks");
    let mut any = false;
    let mut all_current = true;
    for (name, content) in HOOKS {
        match std::fs::read_to_string(hooks.join(name)) {
            Ok(existing) => {
                any = true;
                all_current &= existing.replace("\r\n", "\n") == content.replace("\r\n", "\n");
            }
            Err(_) => all_current = false,
        }
    }
    match (any, all_current) {
        (false, _) => HooksState::Missing,
        (true, false) => HooksState::Outdated,
        (true, true) => HooksState::Current,
    }
}

fn curseforge_file_names(instance: &Value) -> HashSet<String> {
    let mut names = HashSet::new();
    for addon in instance.get("installedAddons").and_then(Value::as_array).into_iter().flatten() {
        for name in [addon.get("fileNameOnDisk"), addon.pointer("/installedFile/fileNameOnDisk"), addon.pointer("/installedFile/fileName")].into_iter().flatten().filter_map(Value::as_str) {
            names.insert(name.to_string());
            names.insert(format!("{name}.disabled"));
        }
    }
    names
}

fn tracked_binaries(dir: &Path, instance: &Value) -> Vec<TrackedBinary> {
    let curseforge = curseforge_file_names(instance);
    let Ok(out) = git::run(dir, &[&["ls-files", "--"][..], &BINARY_PATTERNS].concat()) else { return Vec::new() };
    out.lines()
        .map(|path| {
            let name = path.rsplit('/').next().unwrap_or(path);
            let kind = if path.starts_with("config/spark") {
                BinaryKind::Generated
            } else if curseforge.contains(name) {
                BinaryKind::Curseforge
            } else {
                BinaryKind::Local
            };
            TrackedBinary { path: path.to_string(), kind }
        })
        .collect()
}

fn read_instance(dir: &Path) -> Result<Value> {
    std::fs::read_to_string(dir.join("minecraftinstance.json"))
        .ok()
        .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
        .with_context(|| format!("{} is not a CurseForge instance (no minecraftinstance.json)", dir.display()))
}

pub fn inspect(dir: &Path, added: &[PathBuf]) -> Result<InstanceInfo> {
    let instance = read_instance(dir)?;
    let has_git = dir.join(".git").exists();
    Ok(InstanceInfo {
        dir: dir.to_path_buf(),
        name: instance.get("name").and_then(Value::as_str).map(String::from).unwrap_or_else(|| dir.file_name().unwrap_or_default().to_string_lossy().into_owned()),
        minecraft: instance.get("gameVersion").and_then(Value::as_str).map(String::from),
        loader: instance.pointer("/baseModLoader/name").and_then(Value::as_str).map(String::from),
        addon_count: instance.get("installedAddons").and_then(Value::as_array).map_or(0, Vec::len),
        has_git,
        has_lock: dir.join(LOCKFILE).exists(),
        hooks: hooks_state(dir),
        tracked_binaries: if has_git { tracked_binaries(dir, &instance) } else { Vec::new() },
        remote: if has_git { git::run(dir, &["remote", "get-url", "origin"]).ok().map(|s| s.trim().to_string()) } else { None },
        added: added.iter().any(|p| same_dir(p, dir)),
    })
}

pub fn same_dir(a: &Path, b: &Path) -> bool {
    let normalize = |p: &Path| p.to_string_lossy().trim_end_matches(['\\', '/']).replace('\\', "/").to_lowercase();
    normalize(a) == normalize(b)
}

pub fn discover(root: &Path, added: &[PathBuf]) -> Vec<InstanceInfo> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut found: Vec<InstanceInfo> = entries.flatten().filter(|e| e.path().is_dir()).filter_map(|e| inspect(&e.path(), added).ok()).collect();
    found.sort_by_key(|i| i.name.to_lowercase());
    found
}

fn python() -> Result<String> {
    for candidate in ["python3", "python"] {
        let mut probe = std::process::Command::new(candidate);
        probe.arg("-c").arg("");
        crate::process::hide_window_std(&mut probe);
        if probe.output().is_ok_and(|o| o.status.success()) {
            return Ok(candidate.to_string());
        }
    }
    bail!("Python 3 is required for the git hooks; install it and try again")
}

fn ensure_lines(path: &Path, lines: &[&str]) -> Result<bool> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let present: Vec<&str> = existing.lines().map(str::trim).collect();
    let missing: Vec<&&str> = lines.iter().filter(|l| !present.contains(l)).collect();
    if missing.is_empty() {
        return Ok(false);
    }
    let mut text = existing;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    for line in missing {
        text.push_str(line);
        text.push('\n');
    }
    std::fs::write(path, text)?;
    Ok(true)
}

pub fn install_hooks(dir: &Path) -> Result<()> {
    let hooks = dir.join(".githooks");
    std::fs::create_dir_all(&hooks)?;
    for (name, content) in HOOKS {
        std::fs::write(hooks.join(name), content.replace("\r\n", "\n"))?;
    }
    Ok(())
}

pub async fn setup(dir: PathBuf, options: SetupOptions, log: Log) -> Result<String> {
    let python = python()?;
    if !dir.join(".git").exists() {
        git::run(&dir, &["init", "-b", "main"])?;
        log.info("Created a git repository");
    }

    install_hooks(&dir)?;
    log.info("Installed the sync hooks in .githooks");
    let gitignore = dir.join(".gitignore");
    if gitignore.exists() {
        let binary_block: Vec<&str> = GITIGNORE_TEMPLATE.lines().skip_while(|l| !l.starts_with("# Binaries")).filter(|l| !l.starts_with('#')).chain(["__pycache__/"]).collect();
        if ensure_lines(&gitignore, &binary_block)? {
            log.info("Added the binary folders to the existing .gitignore");
        }
    } else {
        std::fs::write(&gitignore, GITIGNORE_TEMPLATE)?;
        log.info("Wrote a .gitignore for CurseForge instances");
    }
    ensure_lines(&dir.join(".gitattributes"), &GITATTRIBUTES)?;

    if options.untrack_binaries {
        let removable: Vec<String> =
            tracked_binaries(&dir, &read_instance(&dir)?).into_iter().filter(|b| b.kind != BinaryKind::Local).map(|b| b.path).collect();
        for chunk in removable.chunks(100) {
            let mut args = vec!["rm", "--cached", "--quiet", "--"];
            args.extend(chunk.iter().map(String::as_str));
            git::run(&dir, &args)?;
        }
        for path in &removable {
            log.info(format!("Stopped tracking {path}"));
        }
    }

    if !dir.join(CONFIG_FILE).exists() {
        let mut config = PackConfig { author: options.author.clone(), ..Default::default() };
        config.curseforge.project_id = options.project_id;
        std::fs::write(dir.join(CONFIG_FILE), toml::to_string_pretty(&config)?)?;
        log.info("Wrote packraven.toml");
    }

    git::run(&dir, &["config", "core.hooksPath", ".githooks"])?;
    git::run(&dir, &["config", "filter.iris.clean", &format!("{python} .githooks/iris_clean.py")])?;
    let mut adopt = Command::new(&python);
    adopt.current_dir(&dir).arg(".githooks/modsync.py").arg("adopt");
    crate::process::run_logged(adopt, &log).await.context("building mods.lock.json failed")?;

    if let Some(remote) = options.remote.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
        if git::run(&dir, &["remote", "get-url", "origin"]).is_ok() {
            git::run(&dir, &["remote", "set-url", "origin", remote])?;
        } else {
            git::run(&dir, &["remote", "add", "origin", remote])?;
        }
        log.info(format!("Set origin to {remote}"));
    }

    let new_repo = git::run(&dir, &["rev-parse", "-q", "--verify", "HEAD"]).is_err();
    if new_repo {
        git::run(&dir, &["add", "-A"])?;
    } else {
        git::run(&dir, &["add", "--", ".githooks", ".gitignore", ".gitattributes", CONFIG_FILE, LOCKFILE])?;
    }
    for (name, _) in HOOKS {
        git::run(&dir, &["update-index", "--chmod=+x", &format!(".githooks/{name}")])?;
    }
    match options.commit.filter(|_| new_repo) {
        Some(commit) => {
            let message = Some(commit.message.trim()).filter(|m| !m.is_empty()).unwrap_or("Initial");
            let mut args = vec!["commit", "-m", message];
            let author = commit.author.as_deref().map(str::trim).filter(|a| !a.is_empty()).map(String::from);
            if let Some(author) = &author {
                args.push("--author");
                args.push(author);
            }
            git::run(&dir, &args)?;
            log.info(format!("Committed \"{message}\""));
            Ok("Ready".into())
        }
        None => Ok("Ready; the changes are staged for your next commit".into()),
    }
}
