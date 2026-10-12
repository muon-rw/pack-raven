use std::process::Stdio;
use std::sync::Arc;

use anyhow::{bail, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::jobs::Log;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn hide_window_std(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = command;
}

pub fn hide_window(command: &mut Command) {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let _ = command;
}

pub type LineHook = Arc<dyn Fn(&str) + Send + Sync>;

pub fn spawn_logged(command: Command, log: &Log, stdin: bool) -> Result<Child> {
    spawn_watched(command, log, stdin, None)
}

pub fn spawn_watched(mut command: Command, log: &Log, stdin: bool, hook: Option<LineHook>) -> Result<Child> {
    hide_window(&mut command);
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if stdin { Stdio::piped() } else { Stdio::null() })
        .kill_on_drop(true);
    let mut child = command.spawn()?;
    pipe_lines(child.stdout.take(), log.clone(), "out", hook.clone());
    pipe_lines(child.stderr.take(), log.clone(), "err", hook);
    Ok(child)
}

fn pipe_lines<R: tokio::io::AsyncRead + Unpin + Send + 'static>(reader: Option<R>, log: Log, stream: &'static str, hook: Option<LineHook>) {
    let Some(reader) = reader else { return };
    tokio::spawn(async move {
        let mut reader = BufReader::new(reader);
        while let Some(line) = read_line_lossy(&mut reader).await {
            if let Some(hook) = &hook {
                hook(&line);
            }
            log.stream(stream, &line);
        }
    });
}

pub async fn read_line_lossy<R: tokio::io::AsyncBufRead + Unpin>(reader: &mut R) -> Option<String> {
    let mut buffer = Vec::new();
    match reader.read_until(b'\n', &mut buffer).await {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(String::from_utf8_lossy(&buffer).trim_end_matches(['\r', '\n']).to_string()),
    }
}

pub async fn run_logged(command: Command, log: &Log) -> Result<()> {
    let status = spawn_logged(command, log, false)?.wait().await?;
    if !status.success() {
        bail!("process exited with {status}");
    }
    Ok(())
}
