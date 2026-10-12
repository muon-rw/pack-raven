use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::git;
use crate::pack::Pack;

const FOLDERS: [&str; 4] = ["mods", "resourcepacks", "shaderpacks", "datapacks"];
const EXTENSIONS: [&str; 3] = [".jar", ".zip", ".disabled"];

pub struct SyncStatus {
    pub missing: Vec<String>,
    pub unlocked: Vec<String>,
}

pub fn status(pack: &Pack) -> Result<SyncStatus> {
    let tracked: HashSet<String> = git::tracked_files(&pack.dir)?.into_iter().collect();
    let mut on_disk: HashMap<String, u64> = HashMap::new();
    for folder in FOLDERS {
        let Ok(entries) = std::fs::read_dir(pack.dir.join(folder)) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = format!("{folder}/{name}");
            let managed = EXTENSIONS.iter().any(|e| name.ends_with(e)) && !name.ends_with("-dev.jar") && !tracked.contains(&path);
            if managed && let Ok(meta) = entry.metadata() && meta.is_file() {
                on_disk.insert(path, meta.len());
            }
        }
    }
    let missing = pack.lock.files.iter().filter(|e| on_disk.get(&e.path) != Some(&e.size)).map(|e| e.path.clone()).collect();
    let locked: HashSet<&str> = pack.lock.files.iter().map(|e| e.path.as_str()).collect();
    let mut unlocked: Vec<String> = on_disk.into_keys().filter(|p| !locked.contains(p.as_str())).collect();
    unlocked.sort();
    Ok(SyncStatus { missing, unlocked })
}
