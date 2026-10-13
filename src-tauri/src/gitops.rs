use std::path::Path;

use anyhow::{bail, Result};
use serde::Serialize;
use tokio::process::Command;

use crate::git;
use crate::jobs::Log;

const MAX_DIFF_BYTES: usize = 400_000;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub orig_path: Option<String>,
    pub status: char,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub has_commits: bool,
    pub has_remote: bool,
    pub staged: Vec<FileChange>,
    pub unstaged: Vec<FileChange>,
    pub conflicted: Vec<FileChange>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub hash: String,
    pub short: String,
    pub author: String,
    pub when: String,
    pub subject: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    pub text: String,
    pub binary: bool,
    pub truncated: bool,
}

pub fn status(dir: &Path) -> Result<Status> {
    let out = git::run(dir, &["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=all"])?;
    let mut status = Status { has_remote: !git::run(dir, &["remote"])?.trim().is_empty(), ..Default::default() };
    let mut records = out.split('\0').filter(|r| !r.is_empty());
    while let Some(record) = records.next() {
        if let Some(header) = record.strip_prefix("# ") {
            let (key, value) = header.split_once(' ').unwrap_or((header, ""));
            match key {
                "branch.oid" => status.has_commits = value != "(initial)",
                "branch.head" => status.branch = Some(value.to_string()).filter(|v| v != "(detached)"),
                "branch.upstream" => status.upstream = Some(value.to_string()),
                "branch.ab" => {
                    for part in value.split_whitespace() {
                        if let Some(n) = part.strip_prefix('+') {
                            status.ahead = n.parse().unwrap_or(0);
                        } else if let Some(n) = part.strip_prefix('-') {
                            status.behind = n.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let kind = record.chars().next().unwrap_or(' ');
        match kind {
            '?' => status.unstaged.push(FileChange { path: record[2..].to_string(), orig_path: None, status: '?' }),
            '1' | '2' => {
                let fields: Vec<&str> = record.splitn(if kind == '1' { 9 } else { 10 }, ' ').collect();
                let xy: Vec<char> = fields[1].chars().collect();
                let path = fields.last().copied().unwrap_or_default().to_string();
                let orig_path = if kind == '2' { records.next().map(String::from) } else { None };
                if xy[0] != '.' {
                    status.staged.push(FileChange { path: path.clone(), orig_path: orig_path.clone(), status: xy[0] });
                }
                if xy[1] != '.' {
                    status.unstaged.push(FileChange { path, orig_path, status: xy[1] });
                }
            }
            'u' => {
                let path = record.splitn(11, ' ').last().unwrap_or_default().to_string();
                status.conflicted.push(FileChange { path, orig_path: None, status: 'U' });
            }
            _ => {}
        }
    }
    Ok(status)
}

pub fn diff(dir: &Path, path: &str, staged: bool, untracked: bool) -> Result<Diff> {
    let text = if untracked {
        let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
        git::run_accepting(dir, &["diff", "--no-color", "--no-index", "--", null, path], &[0, 1])?
    } else if staged {
        git::run(dir, &["diff", "--no-color", "--cached", "--", path])?
    } else {
        git::run(dir, &["diff", "--no-color", "--", path])?
    };
    let binary = text.lines().any(|l| l.starts_with("Binary files "));
    let truncated = text.len() > MAX_DIFF_BYTES;
    let text = if truncated { text[..text.floor_char_boundary(MAX_DIFF_BYTES)].to_string() } else { text };
    Ok(Diff { text, binary, truncated })
}

pub fn stage(dir: &Path, paths: &[String]) -> Result<()> {
    for chunk in paths.chunks(200) {
        let mut args = vec!["add", "-A", "--"];
        args.extend(chunk.iter().map(String::as_str));
        git::run(dir, &args)?;
    }
    Ok(())
}

pub fn unstage(dir: &Path, paths: &[String]) -> Result<()> {
    let has_head = git::run(dir, &["rev-parse", "-q", "--verify", "HEAD"]).is_ok();
    for chunk in paths.chunks(200) {
        let mut args = if has_head { vec!["restore", "--staged", "--"] } else { vec!["rm", "--cached", "-r", "--quiet", "--"] };
        args.extend(chunk.iter().map(String::as_str));
        git::run(dir, &args)?;
    }
    Ok(())
}

pub fn discard(dir: &Path, tracked: &[String], untracked: &[String]) -> Result<()> {
    for chunk in tracked.chunks(200) {
        let mut args = vec!["restore", "--worktree", "--"];
        args.extend(chunk.iter().map(String::as_str));
        git::run(dir, &args)?;
    }
    for chunk in untracked.chunks(200) {
        let mut args = vec!["clean", "-f", "-q", "--"];
        args.extend(chunk.iter().map(String::as_str));
        git::run(dir, &args)?;
    }
    Ok(())
}

pub fn recent_commits(dir: &Path, count: usize) -> Vec<Commit> {
    let format = "--format=%H%x1f%h%x1f%an%x1f%ar%x1f%s%x1e";
    let Ok(out) = git::run(dir, &["log", &format!("-n{count}"), format]) else { return Vec::new() };
    out.split('\x1e')
        .filter_map(|record| {
            let mut fields = record.trim_start_matches('\n').split('\x1f');
            Some(Commit {
                hash: fields.next().filter(|h| !h.is_empty())?.to_string(),
                short: fields.next()?.to_string(),
                author: fields.next()?.to_string(),
                when: fields.next()?.to_string(),
                subject: fields.next()?.to_string(),
            })
        })
        .collect()
}

async fn git_logged(dir: &Path, args: &[&str], log: &Log) -> Result<()> {
    log.info(format!("git {}", args.join(" ")));
    let mut command = Command::new("git");
    command.current_dir(dir).args(args).env("GIT_TERMINAL_PROMPT", "0");
    crate::process::run_logged(command, log).await
}

pub async fn commit(dir: &Path, message: &str, log: &Log) -> Result<String> {
    let message = message.trim();
    if message.is_empty() {
        bail!("Write a commit message first.");
    }
    git_logged(dir, &["commit", "-m", message], log).await?;
    Ok(format!("Committed \"{}\"", message.lines().next().unwrap_or(message)))
}

pub async fn pull(dir: &Path, log: &Log) -> Result<String> {
    git_logged(dir, &["pull", "--ff-only"], log).await.map_err(|e| {
        e.context("Pull stopped because the branches have diverged or there are conflicting local changes. Commit or discard your changes, then pull again.")
    })?;
    Ok("Pulled".into())
}

pub async fn push(dir: &Path, log: &Log) -> Result<String> {
    let has_upstream = git::run(dir, &["rev-parse", "--abbrev-ref", "@{upstream}"]).is_ok();
    if has_upstream {
        git_logged(dir, &["push", "--follow-tags"], log).await?;
    } else {
        git_logged(dir, &["push", "--follow-tags", "-u", "origin", "HEAD"], log).await?;
    }
    Ok("Pushed".into())
}

pub async fn fetch(dir: &Path, log: &Log) -> Result<String> {
    git_logged(dir, &["fetch", "--prune"], log).await?;
    Ok("Fetched".into())
}
