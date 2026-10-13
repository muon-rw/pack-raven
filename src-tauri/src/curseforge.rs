use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::http::CLIENT;

const CORE_API: &str = "https://api.curseforge.com/v1";
const UPLOAD_API: &str = "https://minecraft.curseforge.com/api";
pub const CORE_KEY_ENV: &str = "CF_DOWNLOAD_TOKEN";
pub const UPLOAD_TOKEN_ENVS: [&str; 2] = ["CURSEFORGE_TOKEN", "CF_TOKEN"];

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CfMod {
    pub id: u64,
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub links: CfLinks,
    #[serde(default)]
    pub logo: Option<CfLogo>,
    #[serde(default)]
    pub authors: Vec<CfAuthor>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CfAuthor {
    pub name: String,
    pub url: String,
    pub avatar_url: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CfLogo {
    pub thumbnail_url: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct CfLinks {
    pub website_url: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CfFile {
    pub id: u64,
    pub mod_id: u64,
    pub display_name: String,
    pub file_name: String,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub file_status: u32,
    #[serde(default)]
    pub is_server_pack: Option<bool>,
    #[serde(default)]
    pub server_pack_file_id: Option<u64>,
}

pub fn core_key() -> Option<String> {
    std::env::var(CORE_KEY_ENV).ok().filter(|k| !k.is_empty())
}

pub fn upload_token() -> Option<String> {
    UPLOAD_TOKEN_ENVS.iter().find_map(|name| std::env::var(name).ok().filter(|k| !k.is_empty()))
}

async fn core_post(path: &str, body: Value) -> Result<Value> {
    let key = core_key().with_context(|| format!("{CORE_KEY_ENV} is not set"))?;
    let response = CLIENT.post(format!("{CORE_API}{path}")).header("x-api-key", key).json(&body).send().await?;
    Ok(response.error_for_status()?.json::<Value>().await?["data"].take())
}

async fn core_get(path: &str) -> Result<Value> {
    let key = core_key().with_context(|| format!("{CORE_KEY_ENV} is not set"))?;
    let response = CLIENT.get(format!("{CORE_API}{path}")).header("x-api-key", key).send().await?;
    Ok(response.error_for_status()?.json::<Value>().await?["data"].take())
}

pub async fn mods(ids: &[u64]) -> Result<HashMap<u64, CfMod>> {
    let mut result = HashMap::new();
    for chunk in ids.chunks(500) {
        let data = core_post("/mods", json!({ "modIds": chunk, "filterPcOnly": true })).await?;
        for m in serde_json::from_value::<Vec<CfMod>>(data)? {
            result.insert(m.id, m);
        }
    }
    Ok(result)
}

pub async fn files(ids: &[u64]) -> Result<HashMap<u64, CfFile>> {
    let mut result = HashMap::new();
    for chunk in ids.chunks(500) {
        let data = core_post("/mods/files", json!({ "fileIds": chunk })).await?;
        for f in serde_json::from_value::<Vec<CfFile>>(data)? {
            result.insert(f.id, f);
        }
    }
    Ok(result)
}

pub async fn file(project_id: u64, file_id: u64) -> Result<Option<CfFile>> {
    match core_get(&format!("/mods/{project_id}/files/{file_id}")).await {
        Ok(data) => Ok(Some(serde_json::from_value(data)?)),
        Err(error) if error.downcast_ref::<reqwest::Error>().and_then(|e| e.status()).is_some_and(|s| s.as_u16() == 404) => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn file_status_name(status: u32) -> &'static str {
    match status {
        1 => "Processing",
        2 => "Changes required",
        3 => "Under review",
        4 => "Approved",
        5 => "Rejected",
        6 => "Malware detected",
        7 => "Deleted",
        8 => "Archived",
        9 => "Testing",
        10 => "Released",
        11 => "Ready for review",
        12 => "Deprecated",
        13 => "Baking",
        14 => "Awaiting publishing",
        15 => "Failed publishing",
        _ => "Unknown",
    }
}

#[derive(Deserialize)]
struct GameVersion {
    id: u64,
    #[serde(rename = "gameVersionTypeID")]
    type_id: u64,
    name: String,
}

#[derive(Deserialize)]
struct GameVersionType {
    id: u64,
    slug: String,
}

pub async fn game_version_ids(minecraft: &str, loader: &str) -> Result<Vec<u64>> {
    let token = upload_token().context("no CurseForge upload token is set (CURSEFORGE_TOKEN)")?;
    let types: Vec<GameVersionType> = CLIENT
        .get(format!("{UPLOAD_API}/game/version-types"))
        .header("X-Api-Token", &token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let versions: Vec<GameVersion> = CLIENT
        .get(format!("{UPLOAD_API}/game/versions"))
        .header("X-Api-Token", &token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let minor: String = minecraft.split('.').take(2).collect::<Vec<_>>().join("-");
    let minecraft_type = types.iter().find(|t| t.slug == format!("minecraft-{minor}")).with_context(|| format!("no CurseForge version type for {minecraft}"))?;
    let modloader_type = types.iter().find(|t| t.slug == "modloader").context("no CurseForge modloader version type")?;
    let find = |type_id: u64, name: &str| versions.iter().find(|v| v.type_id == type_id && v.name.eq_ignore_ascii_case(name)).map(|v| v.id);
    let minecraft_id = find(minecraft_type.id, minecraft).with_context(|| format!("CurseForge has no game version {minecraft}"))?;
    let loader_id = find(modloader_type.id, loader).with_context(|| format!("CurseForge has no loader {loader}"))?;
    Ok(vec![minecraft_id, loader_id])
}

pub async fn upload(project_id: u64, path: &Path, metadata: Value) -> Result<u64> {
    let token = upload_token().context("no CurseForge upload token is set (CURSEFORGE_TOKEN)")?;
    let file_name = path.file_name().context("upload path has no file name")?.to_string_lossy().into_owned();
    let bytes = tokio::fs::read(path).await?;
    let part = reqwest::multipart::Part::bytes(bytes).file_name(file_name).mime_str("application/zip")?;
    let form = reqwest::multipart::Form::new().text("metadata", metadata.to_string()).part("file", part);
    let response = CLIENT
        .post(format!("{UPLOAD_API}/projects/{project_id}/upload-file"))
        .header("X-Api-Token", token)
        .multipart(form)
        .send()
        .await?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        bail!("CurseForge rejected the upload ({status}): {body}");
    }
    body["id"].as_u64().with_context(|| format!("CurseForge upload returned no file id: {body}"))
}
