use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::modsync;
use crate::pack::Pack;

pub struct SyncStatus {
    pub missing: Vec<String>,
    pub unlocked: Vec<String>,
}

pub fn status(pack: &Pack) -> Result<SyncStatus> {
    let on_disk: HashMap<String, u64> = modsync::managed_files(&pack.dir)?
        .into_iter()
        .filter_map(|path| std::fs::metadata(pack.dir.join(&path)).ok().map(|m| (path, m.len())))
        .collect();
    let missing = pack.lock.files.iter().filter(|e| on_disk.get(&e.path) != Some(&e.size)).map(|e| e.path.clone()).collect();
    let locked: HashSet<&str> = pack.lock.files.iter().map(|e| e.path.as_str()).collect();
    let mut unlocked: Vec<String> = on_disk.into_keys().filter(|p| !locked.contains(p.as_str())).collect();
    unlocked.sort();
    Ok(SyncStatus { missing, unlocked })
}
