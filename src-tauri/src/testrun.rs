use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, watch};

use crate::export::{local_dev_jars, server_args};
use crate::jobs::Log;
use crate::launch;
use crate::mirror::{mirror, FilePlan, MirrorReport, Planned};
use crate::pack::Pack;
use crate::process::{spawn_watched, LineHook};
use crate::settings::Settings;

// Fabric keeps its remapped mods in .fabric, and rebuilding that cache slows every start.
const KEPT_ON_RESYNC: [&str; 5] = ["world", "logs", "crash-reports", "usercache.json", ".fabric"];
// We join by IP rather than "localhost", which can resolve to IPv6 first and miss a server bound to IPv4.
const TEST_SERVER_IP: &str = "127.0.0.1";

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

impl TestRun {
    pub fn running(&self) -> bool {
        self.server.lock().unwrap().is_some() || self.client.lock().unwrap().is_some()
    }

    pub async fn kill_all(&self) {
        for slot in [&self.server, &self.client] {
            if let Some(process) = slot.lock().unwrap().as_ref() {
                process.kill();
            }
        }
        // supervise() clears each slot only after the process exits, so we wait for that.
        for _ in 0..100 {
            if !self.running() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
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

pub fn sync_server_dir(mut plan: FilePlan, pack: &Pack, test_dir: &Path, recopy_all: bool, log: &Log) -> Result<MirrorReport> {
    let dev_jars = local_dev_jars(pack);
    for jar in &dev_jars {
        plan.insert(format!("mods/{jar}"), Planned::Copy(pack.dir.join("mods").join(jar)));
    }
    if !dev_jars.is_empty() {
        log.info(format!("Adding local dev builds: {}", dev_jars.join(", ")));
    }
    plan.insert("eula.txt".into(), Planned::Write("eula=true\n".into()));
    if let Some(Planned::Write(properties)) = plan.get_mut("server.properties") {
        *properties = test_properties(properties, pack.config.test.port);
    }
    mirror(test_dir, &plan, &KEPT_ON_RESYNC, recopy_all).context("couldn't update the test server folder. Is a server from an earlier run still open?")
}

// An offline server lets anyone join under any name, so we only listen on this PC.
fn test_properties(text: &str, port: u16) -> String {
    let overridden = ["online-mode=", "server-ip=", "server-port="];
    let mut lines: Vec<String> = text.lines().filter(|l| !overridden.iter().any(|key| l.starts_with(key))).map(String::from).collect();
    lines.push("online-mode=false".into());
    lines.push(format!("server-ip={TEST_SERVER_IP}"));
    lines.push(format!("server-port={port}"));
    lines.join("\n") + "\n"
}

pub async fn run_server(app: AppHandle, run: Arc<TestRun>, pack: &Pack, java: PathBuf, test_dir: PathBuf, ready: watch::Sender<bool>, log: Log) -> Result<String> {
    let mut command = Command::new(&java);
    command.current_dir(&test_dir).args(server_args(pack, &test_dir)?);
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

pub async fn run_client(app: AppHandle, run: Arc<TestRun>, settings: &Settings, pack: &Pack, natives: PathBuf, join: bool, log: Log) -> Result<String> {
    if run.client.lock().unwrap().is_some() {
        bail!("the client is already running");
    }
    let address = join.then(|| format!("{TEST_SERVER_IP}:{}", pack.config.test.port));
    std::fs::create_dir_all(&natives)?;
    let launch = launch::client(settings, pack, &pack.config.test.username, address.as_deref(), &natives)?;
    let target = address.as_deref().map_or_else(|| "to the title screen".to_string(), |a| format!("joining {a}"));
    log.info(format!("Launching {} offline as {}, {target}", pack.lock.loader()?, pack.config.test.username));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_runs_offline_on_this_pc_only() {
        let properties = test_properties("motd=Ravens\nonline-mode=true\nserver-ip=0.0.0.0\nserver-port=1\n", 25565);
        assert_eq!(properties, "motd=Ravens\nonline-mode=false\nserver-ip=127.0.0.1\nserver-port=25565\n");
    }
}
