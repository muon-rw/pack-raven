use std::path::Path;
use std::process::Command;

use anyhow::{bail, Result};

pub fn run(dir: &Path, args: &[&str]) -> Result<String> {
    run_accepting(dir, args, &[0])
}

pub fn run_accepting(dir: &Path, args: &[&str], ok_codes: &[i32]) -> Result<String> {
    let mut command = Command::new("git");
    command.current_dir(dir).args(args).env("GIT_TERMINAL_PROMPT", "0");
    crate::process::hide_window_std(&mut command);
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
