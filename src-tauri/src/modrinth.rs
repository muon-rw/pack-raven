use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::credentials::{self, Credential};
use crate::http::CLIENT;
use crate::pack::ModrinthFile;

const API: &str = "https://api.modrinth.com/v2";
const NO_TOKEN: &str = "no Modrinth token is set; add one in App settings";
// Modrinth's server only accepts mrpack downloads from these hosts. Its docs also list gitlab.com,
// but labrinth's WHITELISTED_MODPACK_DOMAINS doesn't.
pub const PACK_DOWNLOAD_HOSTS: [&str; 3] = ["cdn.modrinth.com", "github.com", "raw.githubusercontent.com"];

#[derive(Deserialize, Clone, Debug)]
pub struct Version {
    pub id: String,
    pub project_id: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub files: Vec<VersionFile>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct VersionFile {
    pub hashes: FileHashes,
    pub url: String,
    pub filename: String,
    pub size: u64,
}

#[derive(Deserialize, Clone, Debug)]
pub struct FileHashes {
    pub sha1: String,
    pub sha512: String,
}

impl Version {
    pub fn lock_file(&self, file: &VersionFile) -> ModrinthFile {
        ModrinthFile {
            project_id: self.project_id.clone(),
            version_id: self.id.clone(),
            sha1: file.hashes.sha1.clone(),
            sha512: file.hashes.sha512.clone(),
            size: file.size,
            url: file.url.clone(),
        }
    }

    pub fn file_with_sha1(&self, sha1: &str) -> Option<&VersionFile> {
        self.files.iter().find(|f| f.hashes.sha1.eq_ignore_ascii_case(sha1))
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct Project {
    pub id: String,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub icon_url: Option<String>,
    pub client_side: String,
    pub server_side: String,
}

impl Project {
    pub fn url(&self) -> String {
        format!("https://modrinth.com/project/{}", self.slug)
    }
}

#[derive(Deserialize)]
pub struct SearchHit {
    pub project_id: String,
}

fn is_not_found(error: &reqwest::Error) -> bool {
    error.status().is_some_and(|s| s.as_u16() == 404)
}

async fn get<T: serde::de::DeserializeOwned>(path: &str, query: &[(&str, String)]) -> Result<Option<T>> {
    match CLIENT.get(format!("{API}{path}")).query(query).send().await?.error_for_status() {
        Ok(response) => Ok(Some(response.json().await?)),
        Err(error) if is_not_found(&error) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub async fn versions_by_sha1(hashes: &[String]) -> Result<HashMap<String, Version>> {
    let mut found = HashMap::new();
    for chunk in hashes.chunks(500) {
        let versions: HashMap<String, Version> = CLIENT
            .post(format!("{API}/version_files"))
            .json(&json!({ "hashes": chunk, "algorithm": "sha1" }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        found.extend(versions);
    }
    Ok(found)
}

/// Returns Modrinth's file for each hash it hosts.
pub async fn files_by_sha1(hashes: &[String]) -> Result<HashMap<String, ModrinthFile>> {
    Ok(versions_by_sha1(hashes)
        .await?
        .into_iter()
        .filter_map(|(sha1, version)| {
            let file = version.file_with_sha1(&sha1)?;
            Some((sha1.to_ascii_lowercase(), version.lock_file(file)))
        })
        .collect())
}

pub async fn projects(ids: &[String]) -> Result<HashMap<String, Project>> {
    let mut found = HashMap::new();
    for chunk in ids.chunks(200) {
        let list: Vec<Project> = get("/projects", &[("ids", serde_json::to_string(chunk)?)]).await?.unwrap_or_default();
        found.extend(list.into_iter().map(|p| (p.id.clone(), p)));
    }
    Ok(found)
}

/// Accepts a project's ID or its slug.
pub async fn project(id: &str) -> Result<Option<Project>> {
    get(&format!("/project/{id}"), &[]).await
}

pub async fn version(id: &str) -> Result<Option<Version>> {
    get(&format!("/version/{id}"), &[]).await
}

pub async fn project_versions(id: &str, loaders: &[&str], minecraft: &str) -> Result<Vec<Version>> {
    let mut query = vec![("game_versions", serde_json::to_string(&[minecraft])?)];
    if !loaders.is_empty() {
        query.push(("loaders", serde_json::to_string(loaders)?));
    }
    Ok(get(&format!("/project/{id}/version"), &query).await?.unwrap_or_default())
}

pub async fn search(text: &str, minecraft: &str, limit: u32) -> Result<Vec<SearchHit>> {
    #[derive(Deserialize)]
    struct Page {
        hits: Vec<SearchHit>,
    }
    let query = [("query", text.to_string()), ("facets", json!([[format!("versions:{minecraft}")]]).to_string()), ("limit", limit.to_string())];
    Ok(get::<Page>("/search", &query).await?.map(|p| p.hits).unwrap_or_default())
}

pub fn token() -> Option<String> {
    credentials::get(Credential::Modrinth)
}

pub async fn create_version(mut data: Value, path: &Path, file_name: String) -> Result<Version> {
    let token = token().context(NO_TOKEN)?;
    data["file_parts"] = json!(["pack"]);
    data["primary_file"] = json!("pack");
    let bytes = tokio::fs::read(path).await?;
    let part = reqwest::multipart::Part::bytes(bytes).file_name(file_name).mime_str("application/x-modrinth-modpack+zip")?;
    let form = reqwest::multipart::Form::new().text("data", data.to_string()).part("pack", part);
    let response = CLIENT.post(format!("{API}/version")).header("Authorization", token).multipart(form).send().await?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let reason = body.get("description").and_then(Value::as_str).map_or_else(|| body.to_string(), String::from);
        bail!("Modrinth rejected the upload ({status}): {reason}");
    }
    serde_json::from_value(body).context("Modrinth's reply to the upload had no version")
}
