use std::collections::HashMap;

use anyhow::Result;
use serde::Deserialize;
use serde_json::json;

use crate::http::CLIENT;

const API: &str = "https://api.modrinth.com/v2";

#[derive(Deserialize)]
struct Version {
    project_id: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Project {
    pub id: String,
    pub client_side: String,
    pub server_side: String,
}

pub async fn projects_by_sha1(hashes: &[String]) -> Result<HashMap<String, Project>> {
    let mut by_hash: HashMap<String, String> = HashMap::new();
    for chunk in hashes.chunks(500) {
        let versions: HashMap<String, Version> = CLIENT
            .post(format!("{API}/version_files"))
            .json(&json!({ "hashes": chunk, "algorithm": "sha1" }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        by_hash.extend(versions.into_iter().map(|(hash, v)| (hash, v.project_id)));
    }
    let mut ids: Vec<&String> = by_hash.values().collect();
    ids.sort();
    ids.dedup();
    let mut projects: HashMap<String, Project> = HashMap::new();
    for chunk in ids.chunks(200) {
        let list: Vec<Project> = CLIENT
            .get(format!("{API}/projects"))
            .query(&[("ids", serde_json::to_string(chunk)?)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        projects.extend(list.into_iter().map(|p| (p.id.clone(), p)));
    }
    Ok(by_hash.into_iter().filter_map(|(hash, id)| projects.get(&id).cloned().map(|p| (hash, p))).collect())
}
