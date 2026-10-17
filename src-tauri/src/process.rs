use std::process::Stdio;
use std::sync::Arc;

use anyhow::{bail, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::jobs::Log;
use crate::logs::Log4jXmlDecoder;

#[cfg(windows)]
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

pub fn encode_powershell(script: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(script.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>())
}

pub fn powershell(script: &str) -> std::process::Command {
    let mut command = std::process::Command::new("powershell");
    command.args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encode_powershell(script)]);
    hide_window_std(&mut command);
    command
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
    bind_to_app(&child);
    pipe_lines(child.stdout.take(), log.clone(), "out", hook.clone());
    pipe_lines(child.stderr.take(), log.clone(), "err", hook);
    Ok(child)
}

#[cfg(windows)]
fn bind_to_app(child: &Child) {
    use std::sync::OnceLock;
    use windows_sys::Win32::System::JobObjects::*;
    static JOB: OnceLock<usize> = OnceLock::new();
    let job = *JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info as *const _ as *const _, size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32);
        job as usize
    });
    if let Some(handle) = child.raw_handle() {
        unsafe { AssignProcessToJobObject(job as _, handle as _) };
    }
}

#[cfg(not(windows))]
fn bind_to_app(_child: &Child) {}

fn pipe_lines<R: tokio::io::AsyncRead + Unpin + Send + 'static>(reader: Option<R>, log: Log, stream: &'static str, hook: Option<LineHook>) {
    let Some(reader) = reader else { return };
    tokio::spawn(async move {
        let mut reader = BufReader::new(reader);
        let mut decoder = Log4jXmlDecoder::default();
        let emit = |line: String| {
            if let Some(hook) = &hook {
                hook(&line);
            }
            log.stream(stream, &line);
        };
        while let Some(line) = read_line_lossy(&mut reader).await {
            decoder.feed(&line).into_iter().for_each(emit);
        }
        decoder.finish().into_iter().for_each(emit);
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
