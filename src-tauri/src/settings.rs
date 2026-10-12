use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::pack::LOCKFILE;
use crate::setup::same_dir;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub instance_dir: PathBuf,
    pub packs: Vec<PathBuf>,
    pub install_dir: PathBuf,
    pub java: Option<PathBuf>,
}

fn curseforge_root() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    if cfg!(target_os = "macos") {
        home.join("Documents").join("curseforge").join("minecraft")
    } else {
        home.join("curseforge").join("minecraft")
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self { instance_dir: PathBuf::new(), packs: Vec::new(), install_dir: curseforge_root().join("Install"), java: None }
    }
}

impl Settings {
    pub fn load(path: &PathBuf) -> Self {
        let mut settings: Self = std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        if settings.packs.is_empty() && settings.instance_dir.join(LOCKFILE).exists() {
            settings.packs.push(settings.instance_dir.clone());
        }
        settings
    }

    pub fn save(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?).context("writing settings")
    }

    pub fn instances_root(&self) -> PathBuf {
        self.install_dir.parent().map(|p| p.join("Instances")).unwrap_or_else(|| curseforge_root().join("Instances"))
    }

    pub fn activate(&mut self, dir: &Path) {
        if !self.packs.iter().any(|p| same_dir(p, dir)) {
            self.packs.push(dir.to_path_buf());
        }
        self.instance_dir = dir.to_path_buf();
    }

    pub fn remove(&mut self, dir: &Path) {
        self.packs.retain(|p| !same_dir(p, dir));
        if same_dir(&self.instance_dir, dir) {
            self.instance_dir = self.packs.first().cloned().unwrap_or_default();
        }
    }
}
