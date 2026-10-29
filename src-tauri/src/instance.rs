use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::modsync;
use crate::pack::{CurseforgeFile, Loader};

const CURSEFORGE_INSTANCE: &str = "minecraftinstance.json";

pub struct Instance {
    pub name: Option<String>,
    pub minecraft: Option<String>,
    pub loader: Option<Loader>,
    /// The launcher's name for the loader's version profile, which launching reads.
    pub version_id: Option<String>,
    pub addon_count: usize,
    /// Names of the files the launcher installed, including their disabled forms.
    pub installed_names: HashSet<String>,
    pub sources_by_sha1: HashMap<String, CurseforgeFile>,
}

// The launcher rewrites these as you add and remove content, so a change to one can change the mod list.
pub fn is_metadata_file(relative: &str) -> bool {
    relative == CURSEFORGE_INSTANCE
}

impl Instance {
    pub fn read(dir: &Path) -> Result<Self> {
        let instance: Value = std::fs::read_to_string(dir.join(CURSEFORGE_INSTANCE))
            .ok()
            .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
            .with_context(|| format!("{} is not a CurseForge instance (no {CURSEFORGE_INSTANCE})", dir.display()))?;
        let addons = instance.get("installedAddons").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        let mut installed_names = HashSet::new();
        let mut sources_by_sha1 = HashMap::new();
        for addon in addons {
            for name in [addon.get("fileNameOnDisk"), addon.pointer("/installedFile/fileNameOnDisk"), addon.pointer("/installedFile/fileName")].into_iter().flatten().filter_map(Value::as_str) {
                installed_names.insert(name.to_string());
                installed_names.insert(format!("{name}.disabled"));
            }
            let installed = &addon["installedFile"];
            let sha1 = installed["hashes"].as_array().into_iter().flatten().find(|h| h["type"] == 1).and_then(|h| h["value"].as_str());
            let (Some(sha1), Some(project_id), Some(file_id)) = (sha1, addon["addonID"].as_u64(), installed["id"].as_u64()) else { continue };
            let url = match installed["downloadUrl"].as_str().filter(|u| !u.is_empty()) {
                Some(url) => url.to_string(),
                None => modsync::cdn_url(file_id, installed["fileName"].as_str().unwrap_or_default()),
            };
            sources_by_sha1.insert(sha1.to_string(), CurseforgeFile { project_id, file_id, url });
        }
        let loader_name = instance.pointer("/baseModLoader/name").and_then(Value::as_str);
        // CurseForge names Fabric loaders fabric-<version>-<minecraft> but installs their profile as
        // fabric-loader-<version>-<minecraft>, so we take the id from the profile CurseForge installed.
        let profile_id = instance
            .pointer("/baseModLoader/versionJson")
            .and_then(Value::as_str)
            .and_then(|json| serde_json::from_str::<Value>(json).ok())
            .and_then(|profile| profile.get("id").and_then(Value::as_str).map(String::from));
        Ok(Self {
            name: instance.get("name").and_then(Value::as_str).map(String::from),
            minecraft: instance.get("gameVersion").and_then(Value::as_str).map(String::from),
            loader: loader_name.and_then(Loader::from_curseforge),
            version_id: profile_id.or(loader_name.map(String::from)),
            addon_count: addons.len(),
            installed_names,
            sources_by_sha1,
        })
    }
}
