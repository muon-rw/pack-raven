use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::jobs::Log;
use crate::pack::Pack;
use crate::settings::Settings;
use crate::sides::{self, MetadataCache, Side};
use crate::{export, launch};

fn scratch() -> PathBuf {
    std::env::var("PACKRAVEN_SCRATCH").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir().join("packraven-tests"))
}

fn instance() -> PathBuf {
    std::env::var("PACKRAVEN_INSTANCE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Settings::default().instances_root().join("Raven's High Fantasy"))
}

fn pack() -> (Settings, Pack) {
    let settings = Settings { instance_dir: instance(), ..Settings::default() };
    let pack = Pack::open(&settings.instance_dir).expect("instance");
    (settings, pack)
}

#[tokio::test]
#[ignore]
async fn setup_fresh_instance() {
    let source = instance();
    let dir = scratch().join("fresh-instance");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("mods")).unwrap();
    std::fs::create_dir_all(dir.join("config")).unwrap();
    std::fs::copy(source.join("minecraftinstance.json"), dir.join("minecraftinstance.json")).unwrap();
    std::fs::copy(source.join("config/fml.toml"), dir.join("config/fml.toml")).unwrap();
    let source_pack = Pack::open(&source).unwrap();
    let jars: Vec<&str> = source_pack.lock.files.iter().filter(|e| e.path.starts_with("mods/") && e.size < 200_000).take(2).map(|e| e.path.as_str()).collect();
    for jar in &jars {
        std::fs::copy(source.join(jar), dir.join(jar)).unwrap();
    }
    std::fs::write(dir.join("mods/not-on-curseforge.jar"), b"local build").unwrap();

    let options = crate::setup::SetupOptions { remote: None, author: "test".into(), project_id: 0, untrack_binaries: true, commit: Some(crate::setup::FirstCommit { message: String::new(), author: None }) };
    let message = crate::setup::setup(dir.clone(), options, &Default::default(), Log::console("setup")).await.unwrap();
    println!("{message}");

    let info = crate::setup::inspect(&dir, &[]).unwrap();
    assert!(info.has_git && info.has_lock && info.hooks == crate::setup::HooksState::Current);
    let pack = Pack::open(&dir).unwrap();
    let locked: Vec<&str> = pack.lock.files.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(locked.len(), jars.len(), "locked {locked:?}");
    let tracked = crate::git::tracked_files(&dir).unwrap();
    assert!(tracked.contains(&"mods/not-on-curseforge.jar".to_string()), "unknown jar should be committed directly");
    assert!(jars.iter().all(|j| !tracked.contains(&j.to_string())), "CurseForge jars must not be tracked");
    assert!(tracked.contains(&"config/fml.toml".to_string()));
    assert_eq!(crate::git::run(&dir, &["status", "--porcelain"]).unwrap().trim(), "", "working tree should be clean");
    assert_eq!(crate::git::run(&dir, &["config", "core.hooksPath"]).unwrap().trim(), ".githooks");
}

#[tokio::test]
#[ignore]
async fn classify_sides() {
    let (_, pack) = pack();
    let mut cache = MetadataCache::load(&scratch().join("metadata-cache.json"));
    let result = sides::classify(&pack, &mut cache).await.unwrap();
    cache.save(&scratch().join("metadata-cache.json")).unwrap();
    for side in [Side::Client, Side::Server] {
        for m in result.iter().filter(|m| m.side == side) {
            println!("{:?} {} ({})", serde_json::to_string(&m.side).unwrap(), m.name, m.reason);
        }
    }
    let both = result.iter().filter(|m| m.side == Side::Both).count();
    println!("total {}, both {both}", result.len());
}

#[test]
#[ignore]
fn client_command() {
    let (settings, pack) = pack();
    let launch = launch::client(&settings, &pack, &launch::Player { name: "Dev".into(), uuid: "0".repeat(32), access_token: "0".into() }, Some("127.0.0.1:25565"), &scratch().join("natives")).unwrap();
    println!("java {}", launch.java.display());
    for arg in &launch.args {
        if arg.len() > 200 {
            println!("  {}... ({} chars)", &arg[..200], arg.len());
        } else {
            println!("  {arg}");
        }
    }
    let classpath = launch.args.iter().skip_while(|a| *a != "-cp").nth(1).unwrap();
    let separator = if cfg!(windows) { ';' } else { ':' };
    let missing: Vec<&str> = classpath.split(separator).filter(|p| !std::path::Path::new(p).exists()).collect();
    assert!(missing.is_empty(), "missing classpath entries: {missing:?}");
}

async fn boots(java: &Path, pack: &Pack, dir: &Path) -> bool {
    let mut command = Command::new(java);
    command.current_dir(dir).args(export::server_args(pack, dir, &[], &[]).unwrap());
    boots_with(command).await
}

async fn boots_with(mut command: Command) -> bool {
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let started = tokio::time::timeout(Duration::from_secs(900), async {
        while let Some(line) = crate::process::read_line_lossy(&mut reader).await {
            if line.contains("ERROR") || line.contains("Exception") || line.contains("Done (") || line.contains("GraalVM") || line.contains("Installing the") || line.contains("Starting the server") {
                println!("{line}");
            }
            if line.contains("Done (") {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    if started {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(b"stop\n").await.unwrap();
        stdin.flush().await.unwrap();
    }
    let _ = tokio::time::timeout(Duration::from_secs(120), child.wait()).await;
    let _ = child.kill().await;
    started
}

#[tokio::test]
#[ignore]
async fn server_pack_boots() {
    let (settings, pack) = pack();
    let log = Log::console("server-pack");
    let java = launch::java(&settings).unwrap();
    let mut cache = MetadataCache::load(&scratch().join("metadata-cache.json"));
    let sides = sides::classify(&pack, &mut cache).await.unwrap();
    let out = scratch().join("build");
    export::server_pack(&pack, &sides, "test", &out, &scratch(), &log).await.unwrap();
    let zip = export::zip_server_pack(&pack, "test", &out, &log).await.unwrap();
    println!("zip {} ({} MB)", zip.display(), std::fs::metadata(&zip).unwrap().len() / 1_000_000);

    let dir = out.join("server");
    let _ = std::fs::remove_dir_all(dir.join("libraries"));
    std::fs::write(dir.join("eula.txt"), "eula=true\n").unwrap();
    std::fs::write(dir.join("java.txt"), java.to_string_lossy().as_bytes()).unwrap();
    let mut bat = Command::new("cmd");
    bat.current_dir(&dir).args(["/c", ".\\run.bat"]);
    assert!(boots_with(bat).await, "server did not finish starting");
}

async fn modless_pack(source: &Path) -> PathBuf {
    let name = source.file_name().unwrap().to_string_lossy();
    let dir = scratch().join(format!("modless-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("mods")).unwrap();
    std::fs::copy(source.join("minecraftinstance.json"), dir.join("minecraftinstance.json")).unwrap();
    let options = crate::setup::SetupOptions { remote: None, author: "test".into(), project_id: 0, untrack_binaries: true, commit: Some(crate::setup::FirstCommit { message: String::new(), author: None }) };
    crate::setup::setup(dir.clone(), options, &Default::default(), Log::console("setup")).await.unwrap();
    dir
}

/// Boots a modless test server for each instance in PACKRAVEN_LOADER_INSTANCES, a comma-separated
/// list of CurseForge instance names, through the same sync a test run uses.
#[tokio::test]
#[ignore]
async fn loader_test_servers_boot() {
    let names = std::env::var("PACKRAVEN_LOADER_INSTANCES").unwrap_or_else(|_| "MMC4Forge,MMC2Fab".into());
    for name in names.split(',') {
        let source = Settings::default().instances_root().join(name);
        let pack = Pack::open(&modless_pack(&source).await).unwrap();
        // The source instance's CurseForge profile picks the Java version its Minecraft needs.
        let java = launch::java(&Settings { instance_dir: source, ..Settings::default() }).unwrap();
        let log = Log::console(name);
        let test_dir = scratch().join(format!("test-server-{name}"));
        let _ = std::fs::remove_dir_all(&test_dir);
        let plan = export::test_server_plan(&pack, &[], "test", &java, &scratch(), &log).await.unwrap();
        println!("{name}: {}", crate::testrun::sync_server_dir(plan, &pack, &test_dir, false, &log).unwrap());
        assert!(boots(&java, &pack, &test_dir).await, "{name} server did not finish starting");

        let plan = export::test_server_plan(&pack, &[], "test", &java, &scratch(), &log).await.unwrap();
        println!("{name} resync: {}", crate::testrun::sync_server_dir(plan, &pack, &test_dir, false, &log).unwrap());
    }
}

/// Boots a modless server pack through its own run scripts: run.bat here, and run.sh in WSL
/// when it's installed. Each script downloads GraalVM on its first start.
#[tokio::test]
#[ignore]
async fn server_pack_scripts_boot() {
    let name = std::env::var("PACKRAVEN_SCRIPT_INSTANCE").unwrap_or_else(|_| "MMC4Forge".into());
    let source = Settings::default().instances_root().join(&name);
    let pack = Pack::open(&modless_pack(&source).await).unwrap();
    let out = scratch().join(format!("scripts-{name}"));
    let dir = export::server_pack(&pack, &[], "test", &out, &scratch(), &Log::console(&name)).await.unwrap();
    std::fs::write(dir.join("eula.txt"), "eula=true\n").unwrap();

    let mut bat = Command::new("cmd");
    bat.current_dir(&dir).args(["/c", ".\\run.bat"]);
    assert!(boots_with(bat).await, "run.bat didn't start the server");

    if std::process::Command::new("wsl").args(["-e", "true"]).status().is_ok_and(|s| s.success()) {
        let _ = std::fs::remove_dir_all(dir.join("world"));
        let mut sh = Command::new("wsl");
        sh.current_dir(&dir).args(["-e", "sh", "./run.sh"]);
        assert!(boots_with(sh).await, "run.sh didn't start the server");
    }
}

#[test]
#[ignore]
fn client_export_manifest() {
    let (_, pack) = pack();
    let zip = export::client_export(&pack, "test", &scratch().join("build"), &Log::console("client-export")).unwrap();
    println!("zip {} ({} KB)", zip.display(), std::fs::metadata(&zip).unwrap().len() / 1000);
}

#[test]
#[ignore]
fn git_status_matches_porcelain() {
    let dir = instance();
    let status = crate::gitops::status(&dir).unwrap();
    println!("branch {:?} upstream {:?} +{} -{} commits {}", status.branch, status.upstream, status.ahead, status.behind, status.has_commits);
    for change in status.staged.iter().take(5) {
        println!("staged {} {}", change.status, change.path);
    }
    for change in status.unstaged.iter().take(5) {
        println!("unstaged {} {}", change.status, change.path);
    }
    let short = crate::git::run(&dir, &["status", "--short", "--untracked-files=all"]).unwrap();
    let staged = short.lines().filter(|l| !l.starts_with(' ') && !l.starts_with('?')).count();
    let unstaged = short.lines().filter(|l| l.as_bytes().get(1).is_some_and(|c| *c != b' ')).count();
    assert_eq!((status.staged.len(), status.unstaged.len()), (staged, unstaged));
    let diff = crate::gitops::diff(&dir, &status.unstaged.first().unwrap().path, false, false).unwrap();
    println!("diff {} bytes, binary {}", diff.text.len(), diff.binary);
    println!("commits: {:?}", crate::gitops::recent_commits(&dir, 3).iter().map(|c| &c.subject).collect::<Vec<_>>());
}

#[test]
#[ignore]
fn defender_probe() {
    let settings = Settings::default();
    let status = crate::defender::status(&[instance(), settings.install_dir]);
    println!("active {} readable {} missing {:?}", status.active, status.exclusions_readable, status.missing);
}

#[tokio::test]
#[ignore]
async fn git_installer_download_matches_checksum() {
    let installer = crate::git_install::download_installer().await.unwrap();
    println!("{} ({} bytes)", installer.display(), std::fs::metadata(&installer).unwrap().len());
    std::fs::remove_file(installer).unwrap();
}

#[test]
#[ignore]
fn detects_java_on_this_computer() {
    for java in crate::java::detect(&Settings::default().install_dir) {
        println!("{:>2} {:<10} {:<18} {:?} {}", java.major, java.version, java.distribution, java.source, java.path.display());
    }
}

#[tokio::test]
#[ignore]
async fn downloads_graalvm() {
    let dir = scratch().join("java");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = crate::java::DOWNLOAD_DIR.set(dir.clone());
    let java = crate::java::download_graalvm(25, |received, total| println!("{received} of {total:?}")).await.unwrap();
    println!("{java:?}");
    assert_eq!(java.major, 25);
    assert!(java.path.starts_with(&dir));
    let output = std::process::Command::new(&java.path).arg("-version").output().unwrap();
    assert!(output.status.success());
}

// Installs an older Apotheosis into a fresh repo, then updates it, which also pulls in its
// required dependencies.
#[tokio::test]
#[ignore]
async fn update_mod_with_dependencies() {
    use crate::mod_updates::{self, ModRequest};
    use crate::modsync::{self, Placement};
    const APOTHEOSIS: u64 = 313970;
    const APOTHEOSIS_8_8_0: u64 = 8826922;

    let dir = scratch().join("mod-updates");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("mods")).unwrap();
    crate::git::run(&dir, &["init", "-q"]).unwrap();
    std::fs::write(dir.join(crate::pack::LOCKFILE), r#"{"minecraft": "1.21.1", "loader": "neoforge-21.1.209", "files": []}"#).unwrap();
    let log = Log::console("mods");

    let old = crate::curseforge::files(&[APOTHEOSIS_8_8_0]).await.unwrap().remove(&APOTHEOSIS_8_8_0).unwrap();
    let entry = mod_updates::lock_entry(&old, "mods", false).unwrap();
    let old_path = entry.path.clone();
    modsync::place(&dir, vec![Placement { entry, replaces: None }], &log).await.unwrap();
    assert!(dir.join(&old_path).exists());

    let pack = Pack::open(&dir).unwrap();
    let checks = mod_updates::check(&pack).await.unwrap();
    println!("{checks:#?}");
    let newer = serde_json::to_value(&checks[0]).unwrap()["newer"].as_array().unwrap().clone();
    let file_id = newer.iter().find(|f| f["releaseType"] == "release").expect("a newer release")["fileId"].as_u64().unwrap();

    let plan = mod_updates::plan(&pack, &[ModRequest { file_id, replaces: Some(old_path.clone()) }]).await.unwrap();
    println!("dependencies {:?}, unavailable {:?}", plan.dependencies, plan.unavailable);
    assert!(!plan.dependencies.is_empty(), "Apotheosis requires Placebo and Apothic Attributes");
    modsync::place(&dir, plan.placements, &log).await.unwrap();

    let lock = Pack::open(&dir).unwrap().lock;
    assert!(!dir.join(&old_path).exists());
    assert!(lock.files.iter().any(|e| e.curseforge.as_ref().is_some_and(|f| f.project_id == APOTHEOSIS && f.file_id == file_id)));
    assert!(lock.files.iter().all(|e| dir.join(&e.path).exists()));
    assert_eq!(lock.files.len(), plan.dependencies.len() + 1);

    let written = std::fs::read_to_string(dir.join(crate::pack::LOCKFILE)).unwrap();
    assert!(modsync::refresh_mod_list(&dir).await.unwrap().is_empty());
    assert_eq!(std::fs::read_to_string(dir.join(crate::pack::LOCKFILE)).unwrap(), written, "the watcher's refresh should agree with what we wrote");
}

#[tokio::test]
#[ignore]
async fn install_mod_from_search() {
    use crate::mod_updates::{self, ModRequest, SearchRequest};
    use crate::modsync;

    let dir = scratch().join("mod-install");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    crate::git::run(&dir, &["init", "-q"]).unwrap();
    std::fs::write(dir.join(crate::pack::LOCKFILE), r#"{"minecraft": "1.21.1", "loader": "neoforge-21.1.209", "files": []}"#).unwrap();
    let pack = Pack::open(&dir).unwrap();

    let search = |text: &str, kind: &str| serde_json::from_value::<SearchRequest>(serde_json::json!({ "text": text, "kind": kind, "categoryId": null, "sort": "popularity", "index": 0 })).unwrap();
    let page = serde_json::to_value(mod_updates::search(&pack, &search("apotheosis", "mods")).await.unwrap()).unwrap();
    let apotheosis = page["results"].as_array().unwrap().iter().find(|r| r["projectId"] == 313970).expect("Apotheosis in the results");
    let file_id = apotheosis["file"]["fileId"].as_u64().expect("a file for 1.21.1 NeoForge");

    let ranked = serde_json::to_value(mod_updates::search(&pack, &serde_json::from_value(serde_json::json!({ "text": "storage drawers", "kind": "mods", "categoryId": null, "sort": "relevance", "index": 0 })).unwrap()).await.unwrap()).unwrap();
    assert_eq!(ranked["results"][0]["name"], "Storage Drawers");
    assert!(ranked["results"].as_array().unwrap().iter().all(|r| !r["file"].is_null()));

    let resource_packs = serde_json::to_value(mod_updates::search(&pack, &search("", "resourcepacks")).await.unwrap()).unwrap();
    assert!(!resource_packs["results"].as_array().unwrap().is_empty(), "resource packs have no loader, so they must still match");

    let plan = mod_updates::plan(&pack, &[ModRequest { file_id, replaces: None }]).await.unwrap();
    assert_eq!(plan.installs, ["Apotheosis"]);
    println!("dependencies {:?}", plan.dependencies);
    assert!(!plan.dependencies.is_empty());
    modsync::place(&dir, plan.placements, &Log::console("mods")).await.unwrap();

    let pack = Pack::open(&dir).unwrap();
    assert_eq!(pack.lock.files.len(), plan.dependencies.len() + 1);
    let again = mod_updates::plan(&pack, &[ModRequest { file_id, replaces: None }]).await;
    assert!(again.is_err(), "installing a project the pack already has should fail");
}
