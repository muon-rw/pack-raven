use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::value::RawValue;
use serde_json::Value;

use crate::pack::Pack;
use crate::setup::same_dir;

const INSTANCE_JSON: &str = "minecraftinstance.json";

pub enum CurseForgeApp {
    Running,
    Closed { database: PathBuf },
}

impl CurseForgeApp {
    pub fn detect() -> Self {
        if agent_running() {
            return Self::Running;
        }
        let database = dirs::config_dir().unwrap_or_default().join("CurseForge").join("agent").join("GameInstances").join("MinecraftGameInstance.json");
        Self::Closed { database }
    }
}

// The CurseForge app runs its agent for as long as the app is open (in the tray too), and the agent
// owns the instance database. When we can't tell, we assume it's running, because writing under a
// running agent is the case that loses locks.
#[cfg(windows)]
fn agent_running() -> bool {
    let mut command = std::process::Command::new("tasklist");
    command.args(["/FI", "IMAGENAME eq Curse.Agent.Host.exe", "/NH"]);
    crate::process::hide_window_std(&mut command);
    command.output().map_or(true, |output| String::from_utf8_lossy(&output.stdout).contains("Curse.Agent.Host"))
}

#[cfg(not(windows))]
fn agent_running() -> bool {
    std::process::Command::new("pgrep").args(["-f", "Curse.Agent.Host"]).output().map_or(true, |output| output.status.code() != Some(1))
}

fn locked_addons(instance: &str) -> Option<BTreeSet<u64>> {
    let instance: Value = serde_json::from_str(instance.trim_start_matches('\u{feff}')).ok()?;
    Some(
        instance["installedAddons"]
            .as_array()?
            .iter()
            .filter(|addon| addon["isLocked"] == true)
            .filter_map(|addon| addon["addonID"].as_u64())
            .collect(),
    )
}

pub fn curseforge_locks(dir: &Path) -> Option<BTreeSet<u64>> {
    locked_addons(&std::fs::read_to_string(dir.join(INSTANCE_JSON)).ok()?)
}

// The CurseForge app owns this file, and parsing and rewriting it would reorder its keys, so we
// change only the one isLocked value in place. Each addon object lists addonID before isLocked,
// and none of the objects between them have an isLocked of their own.
fn set_curseforge_lock(text: &str, project_id: u64, locked: bool) -> Option<String> {
    let addons = text.find("\"installedAddons\"")?;
    let id = format!("\"addonID\":{project_id}");
    let mut from = addons;
    let start = loop {
        let found = from + text[from..].find(&id)?;
        let after = text[found + id.len()..].chars().next()?;
        if after == ',' || after == '}' {
            break found;
        }
        from = found + id.len();
    };
    let key = start + text[start..].find("\"isLocked\":")? + "\"isLocked\":".len();
    let current = if text[key..].starts_with("true") { "true" } else if text[key..].starts_with("false") { "false" } else { return None };
    let wanted = if locked { "true" } else { "false" };
    Some(format!("{}{wanted}{}", &text[..key], &text[key + current.len()..]))
}

fn with_locks(instance: &str, wanted: &BTreeSet<u64>) -> Option<String> {
    let current = locked_addons(instance)?;
    let mut text = instance.to_string();
    for id in current.symmetric_difference(wanted) {
        if let Some(updated) = set_curseforge_lock(&text, *id, wanted.contains(id)) {
            text = updated;
        }
    }
    (text != instance).then_some(text)
}

// CurseForge loads every instance it already knows from this database at startup and regenerates
// minecraftinstance.json from it, so the lock has to change here as well to survive a restart.
fn database_with_locks(database: &str, dir: &Path, wanted: &BTreeSet<u64>) -> Option<String> {
    #[derive(Deserialize)]
    struct Instance {
        #[serde(rename = "installPath")]
        install_path: String,
    }
    let instances: Vec<&RawValue> = serde_json::from_str(database.trim_start_matches('\u{feff}')).ok()?;
    let instance = instances.into_iter().map(RawValue::get).find(|raw| serde_json::from_str::<Instance>(raw).is_ok_and(|i| same_dir(Path::new(&i.install_path), dir)))?;
    let start = instance.as_ptr() as usize - database.as_ptr() as usize;
    let patched = with_locks(instance, wanted)?;
    Some(format!("{}{patched}{}", &database[..start], &database[start + instance.len()..]))
}

/// Merges lock changes made in the CurseForge app since the last sync into packraven.toml, then
/// writes the pack's locks back to the CurseForge app if it's closed. `last_seen` holds the
/// CurseForge locks from the previous sync on this machine, so an unlock in either app sticks.
pub fn sync(pack: &mut Pack, last_seen_path: &Path, app: &CurseForgeApp) -> Result<()> {
    let Some(curseforge) = curseforge_locks(&pack.dir) else { return Ok(()) };
    let last_seen: BTreeSet<u64> = std::fs::read_to_string(last_seen_path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut ours: BTreeSet<u64> = pack.config.updates.locked.iter().copied().collect();
    let before = ours.clone();
    ours.extend(curseforge.difference(&last_seen));
    for id in last_seen.difference(&curseforge) {
        ours.remove(id);
    }
    if ours != before {
        pack.config.updates.locked = ours.iter().copied().collect();
        pack.save_config()?;
    }
    // A running CurseForge rewrites both files from memory, which would undo our write and then
    // look like an unlock on the next sync. We write any pending locks once it's closed.
    if let CurseForgeApp::Closed { database } = app {
        write_curseforge_locks(&pack.dir, database, &ours)?;
    }
    let seen = curseforge_locks(&pack.dir).unwrap_or_default();
    if let Some(parent) = last_seen_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(last_seen_path, serde_json::to_string(&seen)?)?;
    Ok(())
}

fn write_curseforge_locks(dir: &Path, database: &Path, wanted: &BTreeSet<u64>) -> Result<()> {
    if let Ok(text) = std::fs::read_to_string(database)
        && let Some(updated) = database_with_locks(&text, dir, wanted)
    {
        let temp = database.with_extension("packraven-tmp");
        std::fs::write(&temp, updated).context("writing CurseForge's instance database")?;
        std::fs::rename(&temp, database).context("replacing CurseForge's instance database")?;
    }
    let path = dir.join(INSTANCE_JSON);
    if let Some(updated) = with_locks(&std::fs::read_to_string(&path)?, wanted) {
        std::fs::write(&path, updated).context("writing minecraftinstance.json")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const INSTANCE: &str = r#"{"installedModpack":{"addonID":7,"isLocked":false},"installedAddons":[{"addonID":12,"installedFile":{"id":1,"dependencies":[{"addonId":7}]},"isEnabled":true,"isLocked":false},{"addonID":7,"isLocked":true}]}"#;

    #[test]
    fn flips_one_lock_and_leaves_the_rest() {
        let locked = set_curseforge_lock(INSTANCE, 12, true).unwrap();
        assert_eq!(locked, INSTANCE.replacen(r#""isEnabled":true,"isLocked":false"#, r#""isEnabled":true,"isLocked":true"#, 1));
        let unlocked = set_curseforge_lock(INSTANCE, 7, false).unwrap();
        assert!(unlocked.starts_with(r#"{"installedModpack":{"addonID":7,"isLocked":false}"#), "the modpack entry isn't an installed addon");
        assert!(unlocked.ends_with(r#"{"addonID":7,"isLocked":false}]}"#));
        assert!(set_curseforge_lock(INSTANCE, 1, true).is_none(), "a missing addon changes nothing");
    }

    #[test]
    fn patches_only_the_matching_database_instance() {
        let other = r#"{"installPath":"C:\\Instances\\Other\\","installedAddons":[{"addonID":12,"isLocked":false}]}"#;
        let ours = format!(r#"{{"installPath":"C:\\Instances\\Pack\\",{}"#, &INSTANCE[1..]);
        let database = format!("\u{feff}[{other},{ours}]");
        let patched = database_with_locks(&database, Path::new("c:/instances/pack"), &BTreeSet::from([12])).unwrap();
        let instances: Vec<Value> = serde_json::from_str(patched.trim_start_matches('\u{feff}')).unwrap();
        assert_eq!(locked_addons(&instances[0].to_string()).unwrap(), BTreeSet::new());
        assert_eq!(locked_addons(&instances[1].to_string()).unwrap(), BTreeSet::from([12]));
        assert!(patched.starts_with(&format!("\u{feff}[{other},")));
        assert!(database_with_locks(&database, Path::new("c:/instances/missing"), &BTreeSet::from([12])).is_none());
    }

    #[test]
    fn sync_keeps_unlocks_from_either_app() {
        let dir = std::env::temp_dir().join(format!("packraven-locks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(crate::pack::LOCKFILE), r#"{"files": []}"#).unwrap();
        std::fs::write(dir.join(INSTANCE_JSON), format!("\u{feff}{INSTANCE}")).unwrap();
        let seen = dir.join("seen.json");
        let closed = CurseForgeApp::Closed { database: dir.join("MinecraftGameInstance.json") };
        let mut pack = Pack::open(&dir).unwrap();

        sync(&mut pack, &seen, &closed).unwrap();
        assert_eq!(pack.config.updates.locked, [7], "the first sync adopts CurseForge's locks");

        pack.config.updates.locked = vec![7, 12];
        sync(&mut pack, &seen, &CurseForgeApp::Running).unwrap();
        assert_eq!(curseforge_locks(&dir).unwrap(), BTreeSet::from([7]), "we leave a running CurseForge's files alone");
        sync(&mut pack, &seen, &CurseForgeApp::Running).unwrap();
        assert_eq!(pack.config.updates.locked, [7, 12], "a pending lock survives syncs while CurseForge runs");

        pack.config.updates.locked = vec![12];
        sync(&mut pack, &seen, &closed).unwrap();
        assert_eq!(pack.config.updates.locked, [12]);
        assert_eq!(curseforge_locks(&dir).unwrap(), BTreeSet::from([12]), "we write packraven's locks back");

        let text = std::fs::read_to_string(dir.join(INSTANCE_JSON)).unwrap();
        std::fs::write(dir.join(INSTANCE_JSON), set_curseforge_lock(&text, 12, false).unwrap()).unwrap();
        sync(&mut pack, &seen, &closed).unwrap();
        assert!(pack.config.updates.locked.is_empty(), "an unlock in the CurseForge app sticks");
        assert!(std::fs::read_to_string(dir.join(INSTANCE_JSON)).unwrap().starts_with('\u{feff}'));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
