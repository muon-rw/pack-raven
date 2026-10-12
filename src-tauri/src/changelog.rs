use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

use anyhow::Result;
use serde::Serialize;

use crate::curseforge;
use crate::git;
use crate::pack::{LockEntry, Lockfile, Pack, CHANGELOG, LOCKFILE};
use crate::sides::MetadataCache;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModChange {
    pub name: String,
    pub url: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModDiff {
    pub since: Option<String>,
    pub added: Vec<ModChange>,
    pub updated: Vec<ModChange>,
    pub removed: Vec<ModChange>,
}

pub fn read_notes(pack: &Pack) -> String {
    std::fs::read_to_string(pack.dir.join(CHANGELOG)).unwrap_or_default()
}

pub fn write_notes(pack: &Pack, text: &str) -> Result<()> {
    std::fs::write(pack.dir.join(CHANGELOG), text)?;
    Ok(())
}

fn by_project(lock: &Lockfile) -> BTreeMap<u64, &LockEntry> {
    lock.files.iter().map(|e| (e.project_id, e)).collect()
}

pub async fn mod_diff(pack: &Pack, cache: &mut MetadataCache) -> Result<ModDiff> {
    let since = git::last_release_tag(&pack.dir);
    let previous = match &since {
        Some(tag) => git::show(&pack.dir, tag, LOCKFILE).map(|t| Lockfile::parse(&t)).transpose()?.unwrap_or_default(),
        None => return Ok(ModDiff::default()),
    };
    let old = by_project(&previous);
    let new = by_project(&pack.lock);
    let ids: Vec<u64> = old.keys().chain(new.keys()).copied().collect();
    cache.ensure_names(&ids).await?;
    let changed_files: Vec<u64> = new
        .iter()
        .filter_map(|(id, e)| old.get(id).filter(|o| o.file_id != e.file_id).map(|o| [o.file_id, e.file_id]))
        .flatten()
        .collect();
    let file_names: HashMap<u64, String> = if changed_files.is_empty() {
        HashMap::new()
    } else {
        curseforge::files(&changed_files).await?.into_iter().map(|(id, f)| (id, f.display_name)).collect()
    };
    let display = |entry: &LockEntry| {
        file_names.get(&entry.file_id).cloned().unwrap_or_else(|| entry.path.rsplit('/').next().unwrap_or_default().to_string())
    };
    let change = |id: u64, from: Option<String>, to: Option<String>| {
        let name = cache.name(id);
        ModChange { name: name.name, url: name.url, from, to }
    };
    let mut diff = ModDiff { since, ..Default::default() };
    for (id, entry) in &new {
        match old.get(id) {
            None => diff.added.push(change(*id, None, None)),
            Some(previous) if previous.file_id != entry.file_id => diff.updated.push(change(*id, Some(display(previous)), Some(display(entry)))),
            Some(_) => {}
        }
    }
    for id in old.keys().filter(|id| !new.contains_key(id)) {
        diff.removed.push(change(*id, None, None));
    }
    for list in [&mut diff.added, &mut diff.updated, &mut diff.removed] {
        list.sort_by_key(|c| c.name.to_lowercase());
    }
    Ok(diff)
}

pub fn render(notes: &str, diff: &ModDiff) -> String {
    let mut out = notes.trim_end().to_string();
    let link = |c: &ModChange| match &c.url {
        Some(url) => format!("[{}]({url})", c.name),
        None => c.name.clone(),
    };
    if diff.added.is_empty() && diff.updated.is_empty() && diff.removed.is_empty() {
        return out;
    }
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str("## Mod changes\n");
    if !diff.added.is_empty() {
        out.push_str("\n### Added\n");
        for c in &diff.added {
            let _ = writeln!(out, "- {}", link(c));
        }
    }
    if !diff.updated.is_empty() {
        out.push_str("\n### Updated\n");
        for c in &diff.updated {
            let _ = writeln!(out, "- {}: {} to {}", link(c), c.from.as_deref().unwrap_or("?"), c.to.as_deref().unwrap_or("?"));
        }
    }
    if !diff.removed.is_empty() {
        out.push_str("\n### Removed\n");
        for c in &diff.removed {
            let _ = writeln!(out, "- {}", link(c));
        }
    }
    out
}
