use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "packraven.toml";
pub const LOCKFILE: &str = "mods.lock.json";
pub const CHANGELOG: &str = "changelog.md";
pub const CHANGELOG_ARCHIVE: &str = "changelogs";
const BCC_CONFIG: &str = "config/bcc-common.toml";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct PackConfig {
    pub author: String,
    pub curseforge: CurseforgeConfig,
    pub export: ExportConfig,
    pub server: ServerConfig,
    pub test: TestConfig,
    pub discord: DiscordConfig,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct DiscordConfig {
    pub role_id: String,
    pub color: String,
    pub announce_on_approval: bool,
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self { role_id: String::new(), color: "#4d6b2f".into(), announce_on_approval: true }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct CurseforgeConfig {
    pub project_id: u64,
    pub release_type: String,
    pub manual_release: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ExportConfig {
    pub exclude: Vec<String>,
    pub allowed_override_jars: Vec<String>,
    pub recommended_ram_mb: u32,
    pub properties: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ServerConfig {
    pub exclude: Vec<String>,
    pub client_only: Vec<u64>,
    pub keep: Vec<u64>,
    pub memory: String,
    pub properties: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct TestConfig {
    pub username: String,
    pub port: u16,
    pub client_memory: String,
}

impl Default for PackConfig {
    fn default() -> Self {
        Self {
            author: "muon-rw".into(),
            curseforge: CurseforgeConfig::default(),
            export: ExportConfig::default(),
            server: ServerConfig::default(),
            test: TestConfig::default(),
            discord: DiscordConfig::default(),
        }
    }
}

impl Default for CurseforgeConfig {
    fn default() -> Self {
        Self { project_id: 0, release_type: "release".into(), manual_release: false }
    }
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            exclude: [".githooks/**", ".gitignore", ".gitattributes", LOCKFILE, CONFIG_FILE, CHANGELOG, "changelogs/**", "README*"]
                .map(String::from)
                .to_vec(),
            allowed_override_jars: Vec::new(),
            recommended_ram_mb: 8000,
            properties: BTreeMap::new(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            exclude: ["resourcepacks/**", "shaderpacks/**", "options.txt", "config/fancymenu/**", "config/drippyloadingscreen/**"]
                .map(String::from)
                .to_vec(),
            client_only: Vec::new(),
            keep: Vec::new(),
            memory: "6G".into(),
            properties: [("allow-flight", "true"), ("max-tick-time", "60000"), ("difficulty", "normal")]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }
}

impl Default for TestConfig {
    fn default() -> Self {
        Self { username: "Dev".into(), port: 25565, client_memory: "8G".into() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LockEntry {
    pub path: String,
    pub project_id: u64,
    pub file_id: u64,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Lockfile {
    pub minecraft: Option<String>,
    pub loader: Option<String>,
    pub files: Vec<LockEntry>,
}

impl Lockfile {
    pub fn parse(text: &str) -> Result<Self> {
        serde_json::from_str(text).context("parsing mods.lock.json")
    }

    pub fn minecraft(&self) -> Result<&str> {
        self.minecraft.as_deref().context("mods.lock.json has no minecraft version; commit once to regenerate it")
    }

    pub fn loader(&self) -> Result<&str> {
        self.loader.as_deref().context("mods.lock.json has no loader; commit once to regenerate it")
    }

    pub fn neoforge_version(&self) -> Result<&str> {
        let loader = self.loader()?;
        match loader.strip_prefix("neoforge-") {
            Some(version) => Ok(version),
            None => bail!("only NeoForge packs are supported, found {loader}"),
        }
    }
}

pub struct Pack {
    pub dir: PathBuf,
    pub config: PackConfig,
    pub lock: Lockfile,
}

impl Pack {
    pub fn open(dir: &Path) -> Result<Self> {
        if !dir.join(LOCKFILE).exists() {
            bail!("{} has no {LOCKFILE}; set the instance folder in Settings", dir.display());
        }
        let config = match std::fs::read_to_string(dir.join(CONFIG_FILE)) {
            Ok(text) => toml::from_str(&text).context("parsing packraven.toml")?,
            Err(_) => PackConfig::default(),
        };
        let lock = Lockfile::parse(&std::fs::read_to_string(dir.join(LOCKFILE))?)?;
        Ok(Self { dir: dir.to_path_buf(), config, lock })
    }

    pub fn save_config(&self) -> Result<()> {
        std::fs::write(self.dir.join(CONFIG_FILE), toml::to_string_pretty(&self.config)?).context("writing packraven.toml")
    }

    pub fn name(&self) -> String {
        self.bcc_value("modpackName").unwrap_or_else(|| {
            self.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        })
    }

    pub fn version(&self) -> Option<String> {
        self.bcc_value("modpackVersion")
    }

    pub fn set_version(&self, version: &str) -> Result<()> {
        let path = self.dir.join(BCC_CONFIG);
        let text = std::fs::read_to_string(&path).context("reading the Better Compatibility Checker config")?;
        let pattern = bcc_pattern("modpackVersion");
        if !pattern.is_match(&text) {
            bail!("{BCC_CONFIG} has no modpackVersion entry");
        }
        let updated = pattern.replace(&text, |caps: &regex::Captures| format!("{}\"{}\"", &caps[1], version));
        std::fs::write(&path, updated.as_ref())?;
        Ok(())
    }

    fn bcc_value(&self, key: &str) -> Option<String> {
        let text = std::fs::read_to_string(self.dir.join(BCC_CONFIG)).ok()?;
        bcc_pattern(key).captures(&text).map(|caps| caps[2].to_string())
    }

    pub fn file_stem(&self, version: &str) -> String {
        let name: String = self.name().chars().filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-').collect();
        format!("{}-{}", name.trim().replace(' ', "-"), version)
    }

    pub fn git_dir(&self) -> Result<PathBuf> {
        Ok(PathBuf::from(crate::git::run(&self.dir, &["rev-parse", "--absolute-git-dir"])?.trim()))
    }

    pub fn cache_dir(&self) -> Result<PathBuf> {
        Ok(self.git_dir()?.join("modsync").join("cache"))
    }
}

fn bcc_pattern(key: &str) -> Regex {
    Regex::new(&format!(r#"(?m)^(\s*{key}\s*=\s*)"([^"]*)""#)).expect("valid regex")
}
