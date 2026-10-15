use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use anyhow::{bail, Result};

static PROGRAM: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Returns the git executable, which may sit outside PATH right after packraven installs it.
pub fn program() -> PathBuf {
    let mut cached = PROGRAM.lock().unwrap();
    if let Some(found) = cached.as_ref() {
        return found.clone();
    }
    match candidates().into_iter().find(|candidate| version_of(candidate).is_some()) {
        Some(found) => cached.insert(found).clone(),
        None => PathBuf::from("git"),
    }
}

pub fn version() -> Option<String> {
    version_of(&program())
}

fn version_of(program: &Path) -> Option<String> {
    let mut command = Command::new(program);
    command.arg("--version");
    crate::process::hide_window_std(&mut command);
    let output = command.output().ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&output.stdout).trim().trim_start_matches("git version ").to_string())
}

fn candidates() -> Vec<PathBuf> {
    let mut found = vec![PathBuf::from("git")];
    if cfg!(windows) {
        for (var, suffix) in [("ProgramW6432", "Git"), ("ProgramFiles", "Git"), ("LOCALAPPDATA", "Programs/Git")] {
            if let Some(base) = std::env::var_os(var) {
                found.push(PathBuf::from(base).join(suffix).join("cmd").join("git.exe"));
            }
        }
    }
    found
}

pub fn command() -> Command {
    let mut command = Command::new(program());
    crate::process::hide_window_std(&mut command);
    command
}

pub fn run(dir: &Path, args: &[&str]) -> Result<String> {
    run_accepting(dir, args, &[0])
}

pub fn run_accepting(dir: &Path, args: &[&str], ok_codes: &[i32]) -> Result<String> {
    let mut command = command();
    command.current_dir(dir).args(args).env("GIT_TERMINAL_PROMPT", "0");
    let output = command.output()?;
    if !output.status.code().is_some_and(|code| ok_codes.contains(&code)) {
        bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn tracked_files(dir: &Path) -> Result<Vec<String>> {
    Ok(run(dir, &["ls-files", "-z"])?.split('\0').filter(|s| !s.is_empty()).map(String::from).collect())
}

pub fn show(dir: &Path, rev: &str, path: &str) -> Option<String> {
    run(dir, &["show", &format!("{rev}:{path}")]).ok()
}

pub fn last_release_tag(dir: &Path) -> Option<String> {
    run(dir, &["describe", "--tags", "--abbrev=0", "--match", "v*"]).ok().map(|s| s.trim().to_string())
}

pub fn branch(dir: &Path) -> Option<String> {
    run(dir, &["rev-parse", "--abbrev-ref", "HEAD"]).ok().map(|s| s.trim().to_string())
}

pub fn changed_files(dir: &Path) -> Vec<String> {
    run(dir, &["status", "--porcelain"])
        .map(|out| out.lines().map(|l| l.get(3..).unwrap_or_default().trim_matches('"').to_string()).collect())
        .unwrap_or_default()
}

pub fn ahead_behind(dir: &Path) -> Option<(u32, u32)> {
    let out = run(dir, &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"]).ok()?;
    let mut parts = out.split_whitespace().filter_map(|p| p.parse().ok());
    Some((parts.next()?, parts.next()?))
}

#[derive(Default, Clone)]
pub struct Identity {
    pub name: String,
    pub email: String,
}

impl Identity {
    // user.* sets both the author and the committer, and git config fills in any part left empty.
    pub fn config_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        for (key, value) in [("user.name", &self.name), ("user.email", &self.email)] {
            if !value.trim().is_empty() {
                args.push("-c".into());
                args.push(format!("{key}={}", value.trim()));
            }
        }
        args
    }

    pub fn is_usable(&self, dir: &Path) -> bool {
        let mut args = self.config_args();
        args.extend(["var", "GIT_COMMITTER_IDENT"].map(String::from));
        run(dir, &args.iter().map(String::as_str).collect::<Vec<_>>()).is_ok()
    }
}
