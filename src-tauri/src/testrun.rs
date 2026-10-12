use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, watch};

use crate::export::copy_dir;
use crate::jobs::Log;
use crate::launch;
use crate::pack::Pack;
use crate::process::{spawn_watched, LineHook};
use crate::settings::Settings;

const KEPT_ON_RESYNC: [&str; 4] = ["world", "logs", "crash-reports", "usercache.json"];

#[derive(Default)]
pub struct Managed {
    stdin: tokio::sync::Mutex<Option<ChildStdin>>,
    kill: Mutex<Option<oneshot::Sender<()>>>,
}

impl Managed {
    pub async fn send(&self, line: &str) -> Result<()> {
        let mut stdin = self.stdin.lock().await;
        let pipe = stdin.as_mut().context("the server is not running")?;
        pipe.write_all(format!("{line}\n").as_bytes()).await?;
        pipe.flush().await?;
        Ok(())
    }

    pub fn kill(&self) {
        if let Some(kill) = self.kill.lock().unwrap().take() {
            let _ = kill.send(());
        }
    }
}

#[derive(Default)]
pub struct TestRun {
    pub server: Mutex<Option<Arc<Managed>>>,
    pub client: Mutex<Option<Arc<Managed>>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TestEvent {
    kind: &'static str,
    detail: String,
}

fn emit(app: &AppHandle, kind: &'static str, detail: impl Into<String>) {
    let _ = app.emit("test-event", TestEvent { kind, detail: detail.into() });
}

async fn supervise(mut child: Child, managed: Arc<Managed>, slot: &Mutex<Option<Arc<Managed>>>) -> Result<String> {
    *managed.stdin.lock().await = child.stdin.take();
    let (kill_tx, kill_rx) = oneshot::channel();
    *managed.kill.lock().unwrap() = Some(kill_tx);
    let result = tokio::select! {
        status = child.wait() => status.map(|s| format!("Exited with {s}")).map_err(Into::into),
        _ = kill_rx => {
            child.kill().await?;
            Ok("Killed".to_string())
        }
    };
    slot.lock().unwrap().take();
    result
}

pub fn sync_server_dir(built: &Path, test_dir: &Path) -> Result<()> {
    if test_dir.exists() {
        for entry in std::fs::read_dir(test_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if KEPT_ON_RESYNC.contains(&name.as_str()) {
                continue;
            }
            if entry.file_type()?.is_dir() {
                std::fs::remove_dir_all(entry.path())?;
            } else {
                std::fs::remove_file(entry.path())?;
            }
        }
    }
    copy_dir(built, test_dir)
}

fn configure(test_dir: &Path, port: u16) -> Result<()> {
    std::fs::write(test_dir.join("eula.txt"), "eula=true\n")?;
    let path = test_dir.join("server.properties");
    let mut lines: Vec<String> = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with("online-mode=") && !l.starts_with("server-port="))
        .map(String::from)
        .collect();
    lines.push("online-mode=false".into());
    lines.push(format!("server-port={port}"));
    std::fs::write(&path, lines.join("\n") + "\n")?;
    Ok(())
}

pub async fn run_server(app: AppHandle, run: Arc<TestRun>, pack: &Pack, java: PathBuf, test_dir: PathBuf, ready: watch::Sender<bool>, log: Log) -> Result<String> {
    configure(&test_dir, pack.config.test.port)?;
    let args_file = if cfg!(windows) { "win_args.txt" } else { "unix_args.txt" };
    let neoforge = pack.lock.neoforge_version()?;
    let mut command = Command::new(&java);
    command
        .current_dir(&test_dir)
        .arg("@user_jvm_args.txt")
        .arg(format!("@libraries/net/neoforged/neoforge/{neoforge}/{args_file}"))
        .arg("nogui");
    let hook_app = app.clone();
    let hook: LineHook = Arc::new(move |line: &str| {
        if line.contains("Done (") && line.contains(")! For help") {
            let _ = ready.send(true);
            emit(&hook_app, "server-ready", line);
        } else if let Some(index) = line.find(" joined the game") {
            let name = line[..index].rsplit(' ').next().unwrap_or_default();
            emit(&hook_app, "player-joined", name);
        }
    });
    let child = spawn_watched(command, &log, true, Some(hook))?;
    let managed = Arc::new(Managed::default());
    *run.server.lock().unwrap() = Some(managed.clone());
    emit(&app, "server-started", test_dir.to_string_lossy());
    let result = supervise(child, managed, &run.server).await;
    emit(&app, "server-stopped", "");
    result
}

pub async fn run_client(app: AppHandle, run: Arc<TestRun>, settings: &Settings, pack: &Pack, natives: PathBuf, log: Log) -> Result<String> {
    if run.client.lock().unwrap().is_some() {
        bail!("the client is already running");
    }
    let address = format!("localhost:{}", pack.config.test.port);
    std::fs::create_dir_all(&natives)?;
    let launch = launch::client(settings, pack, &pack.config.test.username, Some(&address), &natives)?;
    log.info(format!("Launching {} offline as {}, joining {address}", pack.lock.loader()?, pack.config.test.username));
    let mut command = Command::new(&launch.java);
    command.current_dir(&settings.instance_dir).args(&launch.args);
    let child = spawn_watched(command, &log, false, None)?;
    let managed = Arc::new(Managed::default());
    *run.client.lock().unwrap() = Some(managed.clone());
    emit(&app, "client-started", "");
    let result = supervise(child, managed, &run.client).await;
    emit(&app, "client-stopped", "");
    result
}
