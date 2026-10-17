use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::pack::Pack;

const INSTANCE_JSON: &str = "minecraftinstance.json";

pub fn curseforge_locks(dir: &Path) -> Option<BTreeSet<u64>> {
    let text = std::fs::read_to_string(dir.join(INSTANCE_JSON)).ok()?;
    let instance: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    Some(
        instance["installedAddons"]
            .as_array()?
            .iter()
            .filter(|addon| addon["isLocked"] == true)
            .filter_map(|addon| addon["addonID"].as_u64())
            .collect(),
    )
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

/// Merges lock changes made in the CurseForge app since the last sync into packraven.toml, then
/// writes the pack's locks back to the CurseForge app. `last_seen` holds the CurseForge locks from
/// the previous sync on this machine, so an unlock in either app sticks.
pub fn sync(pack: &mut Pack, last_seen_path: &Path) -> Result<()> {
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
    write_curseforge_locks(&pack.dir, &ours)?;
    let seen = curseforge_locks(&pack.dir).unwrap_or_default();
    if let Some(parent) = last_seen_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(last_seen_path, serde_json::to_string(&seen)?)?;
    Ok(())
}

fn write_curseforge_locks(dir: &Path, wanted: &BTreeSet<u64>) -> Result<()> {
    let Some(current) = curseforge_locks(dir) else { return Ok(()) };
    let path = dir.join(INSTANCE_JSON);
    let mut text = std::fs::read_to_string(&path)?;
    let original = text.clone();
    for id in current.symmetric_difference(wanted) {
        if let Some(updated) = set_curseforge_lock(&text, *id, wanted.contains(id)) {
            text = updated;
        }
    }
    if text != original {
        std::fs::write(&path, text).context("writing minecraftinstance.json")?;
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
    fn sync_keeps_unlocks_from_either_app() {
        let dir = std::env::temp_dir().join(format!("packraven-locks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(crate::pack::LOCKFILE), r#"{"files": []}"#).unwrap();
        std::fs::write(dir.join(INSTANCE_JSON), format!("\u{feff}{INSTANCE}")).unwrap();
        let seen = dir.join("seen.json");
        let mut pack = Pack::open(&dir).unwrap();

        sync(&mut pack, &seen).unwrap();
        assert_eq!(pack.config.updates.locked, [7], "the first sync adopts CurseForge's locks");

        pack.config.updates.locked = vec![12];
        sync(&mut pack, &seen).unwrap();
        assert_eq!(pack.config.updates.locked, [12]);
        assert_eq!(curseforge_locks(&dir).unwrap(), BTreeSet::from([12]), "we write packraven's locks back");

        let text = std::fs::read_to_string(dir.join(INSTANCE_JSON)).unwrap();
        std::fs::write(dir.join(INSTANCE_JSON), set_curseforge_lock(&text, 12, false).unwrap()).unwrap();
        sync(&mut pack, &seen).unwrap();
        assert!(pack.config.updates.locked.is_empty(), "an unlock in the CurseForge app sticks");
        assert!(std::fs::read_to_string(dir.join(INSTANCE_JSON)).unwrap().starts_with('\u{feff}'));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
