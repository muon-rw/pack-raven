use std::collections::BTreeMap;
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
    pub defender_excluded: Vec<PathBuf>,
    pub defender_dismissed: bool,
    pub discord_webhooks: BTreeMap<String, String>,
    /// Files larger than this many megabytes open only in the simplified editor, if you ask.
    pub editor_ask_mb: u32,
    /// Files larger than this many megabytes don't open in the editor at all.
    pub editor_max_mb: u32,
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
        Self { instance_dir: PathBuf::new(), packs: Vec::new(), install_dir: curseforge_root().join("Install"), java: None, defender_excluded: Vec::new(), defender_dismissed: false, discord_webhooks: BTreeMap::new(), editor_ask_mb: (crate::files::Limits::DEFAULT.ask / 1_000_000) as u32, editor_max_mb: (crate::files::Limits::DEFAULT.max / 1_000_000) as u32 }
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

    pub fn editor_limits(&self) -> crate::files::Limits {
        let ask = u64::from(self.editor_ask_mb.max(1)) * 1_000_000;
        crate::files::Limits { ask, max: (u64::from(self.editor_max_mb) * 1_000_000).max(ask) }
    }

    pub fn webhook(&self, pack: &Path) -> Option<String> {
        self.discord_webhooks.get(&webhook_key(pack)).filter(|w| !w.is_empty()).cloned()
    }

    pub fn set_webhook(&mut self, pack: &Path, url: &str) {
        let url = url.trim();
        if url.is_empty() {
            self.discord_webhooks.remove(&webhook_key(pack));
        } else {
            self.discord_webhooks.insert(webhook_key(pack), url.to_string());
        }
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

fn webhook_key(pack: &Path) -> String {
    pack.to_string_lossy().trim_end_matches(['\\', '/']).replace('\\', "/").to_lowercase()
}
