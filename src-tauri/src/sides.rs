use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::pack::{LockEntry, Pack};
use crate::{curseforge, modrinth};

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MetadataCache {
    sides: HashMap<String, SideMetadata>,
    names: HashMap<u64, ProjectName>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct SideMetadata {
    modrinth_client: Option<String>,
    modrinth_server: Option<String>,
    curseforge_tags: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectName {
    pub name: String,
    pub url: Option<String>,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Both,
    Client,
    Server,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Override {
    Auto,
    Client,
    Keep,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModSide {
    pub path: String,
    pub project_id: u64,
    pub name: String,
    pub url: Option<String>,
    pub side: Side,
    pub reason: String,
    pub r#override: Override,
}

impl MetadataCache {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string(self)?)?;
        Ok(())
    }

    pub async fn ensure_names(&mut self, ids: &[u64]) -> Result<()> {
        let missing: Vec<u64> = ids.iter().copied().filter(|id| !self.names.contains_key(id)).collect();
        if missing.is_empty() {
            return Ok(());
        }
        for (id, m) in curseforge::mods(&missing).await? {
            self.names.insert(id, ProjectName { name: m.name, url: m.links.website_url });
        }
        Ok(())
    }

    pub fn name(&self, id: u64) -> ProjectName {
        self.names.get(&id).cloned().unwrap_or(ProjectName { name: format!("Project {id}"), url: None })
    }

    async fn ensure_sides(&mut self, entries: &[&LockEntry]) -> Result<()> {
        let missing: Vec<&LockEntry> = entries.iter().copied().filter(|e| !self.sides.contains_key(&e.sha1)).collect();
        if missing.is_empty() {
            return Ok(());
        }
        let hashes: Vec<String> = missing.iter().map(|e| e.sha1.clone()).collect();
        let from_modrinth = modrinth::projects_by_sha1(&hashes).await?;
        let file_ids: Vec<u64> = missing.iter().map(|e| e.file_id).collect();
        let from_curseforge = curseforge::files(&file_ids).await?;
        for entry in missing {
            let project = from_modrinth.get(&entry.sha1);
            self.sides.insert(
                entry.sha1.clone(),
                SideMetadata {
                    modrinth_client: project.map(|p| p.client_side.clone()),
                    modrinth_server: project.map(|p| p.server_side.clone()),
                    curseforge_tags: from_curseforge.get(&entry.file_id).map(|f| f.game_versions.clone()).unwrap_or_default(),
                },
            );
        }
        Ok(())
    }
}

pub fn is_server_candidate(entry: &LockEntry) -> bool {
    entry.path.starts_with("mods/") && entry.path.ends_with(".jar")
}

pub async fn classify(pack: &Pack, cache: &mut MetadataCache) -> Result<Vec<ModSide>> {
    let entries: Vec<&LockEntry> = pack.lock.files.iter().filter(|e| is_server_candidate(e)).collect();
    cache.ensure_sides(&entries).await?;
    cache.ensure_names(&entries.iter().map(|e| e.project_id).collect::<Vec<_>>()).await?;
    let config = &pack.config.server;
    let mut result: Vec<ModSide> = entries
        .into_iter()
        .map(|entry| {
            let metadata = cache.sides.get(&entry.sha1).cloned().unwrap_or_default();
            let (detected, reason) = detect(&metadata);
            let (side, reason, r#override) = if config.client_only.contains(&entry.project_id) {
                (Side::Client, "Marked client-only".to_string(), Override::Client)
            } else if config.keep.contains(&entry.project_id) {
                (Side::Both, "Kept on the server".to_string(), Override::Keep)
            } else {
                (detected, reason, Override::Auto)
            };
            let name = cache.name(entry.project_id);
            ModSide { path: entry.path.clone(), project_id: entry.project_id, name: name.name, url: name.url, side, reason, r#override }
        })
        .collect();
    result.sort_by_key(|m| m.name.to_lowercase());
    Ok(result)
}

fn detect(metadata: &SideMetadata) -> (Side, String) {
    match (metadata.modrinth_client.as_deref(), metadata.modrinth_server.as_deref()) {
        (_, Some("unsupported")) => return (Side::Client, "Modrinth: server unsupported".into()),
        (Some("unsupported"), _) => return (Side::Server, "Modrinth: client unsupported".into()),
        (Some(_), Some(_)) => return (Side::Both, "Modrinth: runs on both".into()),
        _ => {}
    }
    let client = metadata.curseforge_tags.iter().any(|t| t == "Client");
    let server = metadata.curseforge_tags.iter().any(|t| t == "Server");
    match (client, server) {
        (true, false) => (Side::Client, "CurseForge: tagged Client only".into()),
        (false, true) => (Side::Server, "CurseForge: tagged Server only".into()),
        (true, true) => (Side::Both, "CurseForge: tagged Client and Server".into()),
        (false, false) => (Side::Both, "No side metadata".into()),
    }
}
