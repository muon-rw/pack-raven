use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "packraven.toml";
pub const LOCKFILE: &str = "mods.lock.json";
// Bump this whenever older packraven versions would misread the new mods.lock.json. Old versions will
// refuse a newer format rather than rewrite it in theirs, so the user will be prompted for an update.
pub const LOCK_FORMAT: u64 = 2;
const NEWER_LOCK_FORMAT: &str = "A newer version of packraven wrote this pack's mods.lock.json. Update packraven in App settings, then try again.";
pub const CHANGELOG: &str = "changelog.md";
pub const CHANGELOG_ARCHIVE: &str = "changelogs";
pub const SERVER_ICON: &str = "server-icon.png";
const BCC_CONFIG: &str = "config/bcc-common.toml";
pub const BCC_PROJECT_ID: u64 = 551894;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct PackConfig {
    pub author: String,
    pub curseforge: CurseforgeConfig,
    pub export: ExportConfig,
    pub server: ServerConfig,
    pub test: TestConfig,
    pub discord: DiscordConfig,
    pub updates: UpdatesConfig,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct UpdatesConfig {
    pub channel: ReleaseType,
    pub locked: Vec<ModKey>,
}

impl Default for UpdatesConfig {
    fn default() -> Self {
        Self { channel: ReleaseType::Alpha, locked: Vec::new() }
    }
}

// Ordered from most to least stable, matching CurseForge's numbering.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseType {
    Release = 1,
    Beta = 2,
    Alpha = 3,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct DiscordConfig {
    pub role_id: String,
    pub color: String,
    pub announce_on_approval: bool,
    pub title: String,
    pub show_author: bool,
    pub header: String,
    pub footer: String,
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self { role_id: String::new(), color: "#4d6b2f".into(), announce_on_approval: true, title: "{project_name} - v{version}".into(), show_author: true, header: String::new(), footer: String::new() }
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
    pub client_only: Vec<ModKey>,
    pub keep: Vec<ModKey>,
    pub memory: String,
    pub jvm_args: Vec<String>,
    pub args: Vec<String>,
    pub graalvm: String,
    pub properties: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct TestConfig {
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
            updates: UpdatesConfig::default(),
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
            jvm_args: Vec::new(),
            args: Vec::new(),
            graalvm: String::new(),
            properties: [("allow-flight", "true"), ("max-tick-time", "60000"), ("difficulty", "normal")]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }
}

impl Default for TestConfig {
    fn default() -> Self {
        Self { port: 25565, client_memory: "8G".into() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ModKey {
    Curseforge(u64),
    Modrinth(String),
}

impl ModKey {
    pub fn curseforge_id(&self) -> Option<u64> {
        match self {
            ModKey::Curseforge(id) => Some(*id),
            ModKey::Modrinth(_) => None,
        }
    }
}

impl std::fmt::Display for ModKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModKey::Curseforge(id) => write!(f, "cf:{id}"),
            ModKey::Modrinth(id) => write!(f, "mr:{id}"),
        }
    }
}

impl std::str::FromStr for ModKey {
    type Err = String;

    fn from_str(text: &str) -> std::result::Result<Self, String> {
        let invalid = || format!("{text} isn't a mod key like cf:238222 or mr:AANobbMI");
        match text.split_once(':') {
            Some(("cf", id)) => id.parse().map(ModKey::Curseforge).map_err(|_| invalid()),
            Some(("mr", id)) if !id.is_empty() => Ok(ModKey::Modrinth(id.to_string())),
            // packraven 0.2.1 and earlier knew mods only by CurseForge project.
            None => text.parse().map(ModKey::Curseforge).map_err(|_| invalid()),
            _ => Err(invalid()),
        }
    }
}

// packraven.toml lists CurseForge projects as bare numbers, which every packraven version reads, so we
// keep writing them that way and only prefix Modrinth's.
impl Serialize for ModKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            ModKey::Curseforge(id) => serializer.serialize_u64(*id),
            ModKey::Modrinth(_) => serializer.collect_str(self),
        }
    }
}

impl<'de> Deserialize<'de> for ModKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Stored {
            Number(u64),
            Text(String),
        }
        match Stored::deserialize(deserializer)? {
            Stored::Number(id) => Ok(ModKey::Curseforge(id)),
            Stored::Text(text) => text.parse().map_err(serde::de::Error::custom),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Loader {
    pub name: String,
    pub version: String,
}

impl Loader {
    // CurseForge names loader profiles like neoforge-21.1.259, and Fabric ones like fabric-0.19.2-1.21.1.
    pub fn from_curseforge(id: &str) -> Option<Self> {
        let (name, rest) = id.split_once('-')?;
        let name = name.to_ascii_lowercase();
        let version = match name.as_str() {
            "fabric" | "quilt" => rest.split_once('-').map_or(rest, |(version, _)| version),
            _ => rest,
        };
        Some(Self { name, version: version.to_string() })
    }
}

impl std::fmt::Display for Loader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.name, self.version)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LockEntry {
    pub path: String,
    pub sha1: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curseforge: Option<CurseforgeFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modrinth: Option<ModrinthFile>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CurseforgeFile {
    pub project_id: u64,
    pub file_id: u64,
    pub url: String,
}

// In a pack on both platforms, the jar on disk is CurseForge's. Modrinth's copy of the same release can
// differ byte for byte, so we record Modrinth's file in full.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModrinthFile {
    pub project_id: String,
    pub version_id: String,
    pub sha1: String,
    pub sha512: String,
    pub size: u64,
    pub url: String,
}

impl LockEntry {
    pub fn curseforge(path: String, sha1: String, size: u64, file: CurseforgeFile) -> Self {
        Self { path, sha1, size, curseforge: Some(file), modrinth: None }
    }

    pub fn key(&self) -> Option<ModKey> {
        match (&self.curseforge, &self.modrinth) {
            (Some(file), _) => Some(ModKey::Curseforge(file.project_id)),
            (None, Some(file)) => Some(ModKey::Modrinth(file.project_id.clone())),
            (None, None) => None,
        }
    }

    pub fn has_key(&self, key: &ModKey) -> bool {
        match key {
            ModKey::Curseforge(id) => self.curseforge.as_ref().is_some_and(|f| f.project_id == *id),
            ModKey::Modrinth(id) => self.modrinth.as_ref().is_some_and(|f| f.project_id == *id),
        }
    }

    pub fn curseforge_project(&self) -> Option<u64> {
        self.curseforge.as_ref().map(|f| f.project_id)
    }

    pub fn download_url(&self) -> Option<&str> {
        match (&self.curseforge, &self.modrinth) {
            (Some(file), _) => Some(&file.url),
            (None, Some(file)) if file.sha1 == self.sha1 => Some(&file.url),
            _ => None,
        }
    }

    // Baselines from CurseForge exports carry no hashes, only file IDs.
    pub fn same_file(&self, other: &LockEntry) -> bool {
        match (&self.curseforge, &other.curseforge) {
            (Some(mine), Some(theirs)) => mine.file_id == theirs.file_id,
            _ => self.sha1 == other.sha1,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(try_from = "serde_json::Value", into = "StoredLockfile")]
pub struct Lockfile {
    pub minecraft: Option<String>,
    pub loader: Option<Loader>,
    pub files: Vec<LockEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredLockfile {
    format_version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    minecraft: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    loader: Option<Loader>,
    files: Vec<LockEntry>,
}

impl From<Lockfile> for StoredLockfile {
    fn from(lock: Lockfile) -> Self {
        Self { format_version: LOCK_FORMAT, minecraft: lock.minecraft, loader: lock.loader, files: lock.files }
    }
}

// packraven 0.2.1 and earlier wrote format 1: CurseForge files only, with CurseForge's loader profile name.
#[derive(Deserialize)]
struct LockfileV1 {
    minecraft: Option<String>,
    loader: Option<String>,
    files: Vec<LockEntryV1>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockEntryV1 {
    path: String,
    project_id: u64,
    file_id: u64,
    sha1: String,
    size: u64,
    url: String,
}

impl From<LockfileV1> for Lockfile {
    fn from(lock: LockfileV1) -> Self {
        let files = lock
            .files
            .into_iter()
            .map(|e| LockEntry::curseforge(e.path, e.sha1, e.size, CurseforgeFile { project_id: e.project_id, file_id: e.file_id, url: e.url }))
            .collect();
        Self { minecraft: lock.minecraft, loader: lock.loader.as_deref().and_then(Loader::from_curseforge), files }
    }
}

impl TryFrom<serde_json::Value> for Lockfile {
    type Error = anyhow::Error;

    fn try_from(value: serde_json::Value) -> Result<Self> {
        // Lockfiles from before we recorded a format are format 1.
        match value.get("formatVersion").map_or(Some(1), serde_json::Value::as_u64) {
            Some(1) => Ok(serde_json::from_value::<LockfileV1>(value).context("parsing mods.lock.json")?.into()),
            Some(LOCK_FORMAT) => {
                let stored: StoredLockfile = serde_json::from_value(value).context("parsing mods.lock.json")?;
                Ok(Self { minecraft: stored.minecraft, loader: stored.loader, files: stored.files })
            }
            _ => bail!(NEWER_LOCK_FORMAT),
        }
    }
}

impl Lockfile {
    pub fn parse(text: &str) -> Result<Self> {
        Self::try_from(serde_json::from_str::<serde_json::Value>(text).context("parsing mods.lock.json")?)
    }

    pub fn minecraft(&self) -> Result<&str> {
        self.minecraft.as_deref().context("mods.lock.json has no minecraft version; commit once so the git hooks regenerate it")
    }

    pub fn loader(&self) -> Result<&Loader> {
        self.loader.as_ref().context("mods.lock.json has no loader; commit once so the git hooks regenerate it")
    }

    pub fn mod_loader(&self) -> Result<ModLoader<'_>> {
        let loader = self.loader()?;
        let version = loader.version.as_str();
        Ok(match loader.name.as_str() {
            "neoforge" => ModLoader::NeoForge(version),
            "forge" => ModLoader::Forge(version),
            "fabric" => ModLoader::Fabric(version),
            _ => bail!("packraven supports NeoForge, Forge and Fabric packs, found {loader}"),
        })
    }
}

#[derive(Debug, PartialEq)]
pub enum ModLoader<'a> {
    NeoForge(&'a str),
    Forge(&'a str),
    Fabric(&'a str),
}

impl ModLoader<'_> {
    pub fn name(&self) -> &'static str {
        match self {
            ModLoader::NeoForge(_) => "NeoForge",
            ModLoader::Forge(_) => "Forge",
            ModLoader::Fabric(_) => "Fabric",
        }
    }

    pub fn manifest_id(&self) -> String {
        match self {
            ModLoader::NeoForge(version) => format!("neoforge-{version}"),
            ModLoader::Forge(version) => format!("forge-{version}"),
            ModLoader::Fabric(version) => format!("fabric-{version}"),
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
            bail!("{} has no {LOCKFILE}; add the pack again from Add pack", dir.display());
        }
        // A newer packraven may have written both files, and only the lockfile's format tells you to update.
        let lock = Lockfile::parse(&std::fs::read_to_string(dir.join(LOCKFILE))?)?;
        let config = match std::fs::read_to_string(dir.join(CONFIG_FILE)) {
            Ok(text) => toml::from_str(&text).context("parsing packraven.toml")?,
            Err(_) => PackConfig::default(),
        };
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
        if let Some(problem) = version_problem(version) {
            bail!("{problem}");
        }
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

    pub fn has_bcc(&self) -> bool {
        self.lock.files.iter().any(|e| e.curseforge_project() == Some(BCC_PROJECT_ID) && !e.path.ends_with(".disabled"))
    }

    // BCC only writes its config the first time the game loads it, so we write one up front to
    // give the version somewhere to live before then. BCC fills in any keys we leave out.
    pub fn write_bcc_config_if_missing(&self, version: &str) -> Result<()> {
        let path = self.dir.join(BCC_CONFIG);
        if path.exists() {
            return Ok(());
        }
        let name = toml::Value::String(self.name()).to_string();
        let version = toml::Value::String(version.into()).to_string();
        std::fs::create_dir_all(path.parent().context("the BCC config path has no parent")?)?;
        std::fs::write(
            &path,
            format!(
                "#General settings\n[general]\n\t#The name of the modpack\n\tmodpackName = {name}\n\t#The version of the modpack\n\tmodpackVersion = {version}\n\t#Use the metadata.json to determine the modpack version\n\t#ONLY ENABLE THIS IF YOU KNOW WHAT YOU ARE DOING\n\tuseMetadata = false\n"
            ),
        )
        .with_context(|| format!("writing {BCC_CONFIG}"))
    }

    fn bcc_value(&self, key: &str) -> Option<String> {
        let text = std::fs::read_to_string(self.dir.join(BCC_CONFIG)).ok()?;
        bcc_pattern(key).captures(&text).map(|caps| caps[2].to_string())
    }

    fn file_stem(&self, version: &str) -> String {
        let name: String = self.name().chars().filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-').collect();
        format!("{}-{}", name.trim().replace(' ', "-"), version)
    }

    pub fn client_zip_name(&self, version: &str) -> String {
        format!("{}.zip", self.file_stem(version))
    }

    pub fn server_zip_name(&self, version: &str) -> String {
        format!("{}-server.zip", self.file_stem(version))
    }

    pub fn git_dir(&self) -> Result<PathBuf> {
        Ok(PathBuf::from(crate::git::run(&self.dir, &["rev-parse", "--absolute-git-dir"])?.trim()))
    }

    pub fn cache_dir(&self) -> Result<PathBuf> {
        Ok(self.git_dir()?.join("modsync").join("cache"))
    }
}

// Publishing uploads to CurseForge before it tags v<version> and writes changelogs/<version>.md, so we
// reject anything git won't take as a tag or Windows won't take as a file name before it gets that far.
// That also keeps quotes out of the BCC config, which set_version writes without escaping.
pub fn version_problem(version: &str) -> Option<String> {
    const UNSAFE: &[char] = &['~', '^', ':', '?', '*', '[', '\\', '"', '<', '>', '|', '/'];
    let version = version.trim();
    if version.is_empty() {
        Some("Enter a version.".into())
    } else if version.chars().any(|c| c.is_whitespace() || c.is_control()) {
        Some("Versions can't contain spaces.".into())
    } else if version.contains(UNSAFE) {
        Some("Versions can't contain any of ~ ^ : ? * [ \\ \" < > | /.".into())
    } else if version.contains("..") || version.contains("@{") {
        Some("Versions can't contain .. or @{.".into())
    } else if version.ends_with('.') || version.ends_with(".lock") {
        Some("Versions can't end in . or .lock.".into())
    } else {
        None
    }
}

fn bcc_pattern(key: &str) -> Regex {
    Regex::new(&format!(r#"(?m)^(\s*{key}\s*=\s*)"([^"]*)""#)).expect("valid regex")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock(loader: &str) -> Lockfile {
        Lockfile { minecraft: Some("1.21.1".into()), loader: Loader::from_curseforge(loader), files: vec![] }
    }

    #[test]
    fn parses_curseforge_loader_ids() {
        assert_eq!(lock("neoforge-21.1.259").mod_loader().unwrap(), ModLoader::NeoForge("21.1.259"));
        assert_eq!(lock("forge-47.4.13").mod_loader().unwrap(), ModLoader::Forge("47.4.13"));
        assert_eq!(lock("fabric-0.19.2-1.21.1").mod_loader().unwrap(), ModLoader::Fabric("0.19.2"));
        assert_eq!(lock("fabric-0.19.2-1.21.1").mod_loader().unwrap().manifest_id(), "fabric-0.19.2");
        assert!(lock("quilt-0.26.0-1.21.1").mod_loader().is_err());
    }

    #[test]
    fn refuses_lockfiles_from_a_newer_format() {
        assert!(Lockfile::parse(r#"{"files": []}"#).is_ok());
        assert!(Lockfile::parse(&format!(r#"{{"formatVersion": {LOCK_FORMAT}, "files": []}}"#)).is_ok());
        for newer in [format!("{}", LOCK_FORMAT + 1), r#""2""#.into()] {
            let error = Lockfile::parse(&format!(r#"{{"formatVersion": {newer}, "entries": []}}"#)).unwrap_err();
            assert_eq!(error.to_string(), NEWER_LOCK_FORMAT, "{newer}");
        }
    }

    #[test]
    fn migrates_format_1_lockfiles() {
        let lock = Lockfile::parse(
            r#"{"minecraft": "1.21.1", "loader": "fabric-0.19.2-1.21.1", "files": [{"path": "mods/a.jar", "projectId": 7, "fileId": 9, "sha1": "abc", "size": 3, "url": "https://example.com/a.jar"}]}"#,
        )
        .unwrap();
        assert_eq!(lock.loader, Some(Loader { name: "fabric".into(), version: "0.19.2".into() }));
        let file = CurseforgeFile { project_id: 7, file_id: 9, url: "https://example.com/a.jar".into() };
        assert_eq!(lock.files, [LockEntry::curseforge("mods/a.jar".into(), "abc".into(), 3, file)]);
        assert_eq!(lock.files[0].key(), Some(ModKey::Curseforge(7)));
        assert_eq!(lock.files[0].download_url(), Some("https://example.com/a.jar"));
    }

    #[test]
    fn round_trips_the_current_format() {
        let modrinth = ModrinthFile { project_id: "AANobbMI".into(), version_id: "v".into(), sha1: "other".into(), sha512: "x".into(), size: 4, url: "https://cdn.modrinth.com/a.jar".into() };
        let entry = LockEntry { modrinth: Some(modrinth), ..LockEntry::curseforge("mods/a.jar".into(), "abc".into(), 3, CurseforgeFile { project_id: 7, file_id: 9, url: String::new() }) };
        let lock = Lockfile { minecraft: Some("1.21.1".into()), loader: Loader::from_curseforge("neoforge-21.1.259"), files: vec![entry] };
        let text = serde_json::to_string(&lock).unwrap();
        assert!(text.starts_with(&format!(r#"{{"formatVersion":{LOCK_FORMAT},"#)), "{text}");
        let read = Lockfile::parse(&text).unwrap();
        assert_eq!((read.minecraft, read.loader, read.files), (lock.minecraft, lock.loader, lock.files));
    }

    #[test]
    fn modrinth_only_entries_download_from_modrinth_when_the_jar_is_theirs() {
        let modrinth = ModrinthFile { project_id: "AANobbMI".into(), version_id: "v".into(), sha1: "abc".into(), sha512: "x".into(), size: 3, url: "https://cdn.modrinth.com/a.jar".into() };
        let entry = LockEntry { path: "mods/a.jar".into(), sha1: "abc".into(), size: 3, curseforge: None, modrinth: Some(modrinth) };
        assert_eq!(entry.key(), Some(ModKey::Modrinth("AANobbMI".into())));
        assert_eq!(entry.download_url(), Some("https://cdn.modrinth.com/a.jar"));
        assert_eq!(LockEntry { sha1: "different".into(), ..entry }.download_url(), None);
    }

    #[test]
    fn keeps_curseforge_keys_as_numbers_in_packraven_toml() {
        let mut config = PackConfig::default();
        config.server.client_only = vec![ModKey::Curseforge(238222), ModKey::Modrinth("AANobbMI".into())];
        let text = toml::to_string(&config).unwrap();
        assert!(text.contains(r#"client_only = [238222, "mr:AANobbMI"]"#), "{text}");
        let read: PackConfig = toml::from_str(&text).unwrap();
        assert_eq!(read.server.client_only, config.server.client_only);
        assert_eq!("cf:5".parse(), Ok(ModKey::Curseforge(5)));
        assert_eq!("5".parse(), Ok(ModKey::Curseforge(5)));
        assert!("mr:".parse::<ModKey>().is_err());
        assert_eq!(ModKey::Modrinth("AANobbMI".into()).to_string(), "mr:AANobbMI");
    }

    #[test]
    fn accepts_free_text_versions_that_work_as_tags_and_file_names() {
        for version in ["1.0.0", "2026.10-winter", "1.0-beta+3", "v2", "R7_hotfix"] {
            assert_eq!(version_problem(version), None, "{version}");
        }
        for version in ["", "1.0 beta", "1.0\"beta\"", "1.0/2", "1..2", "1.0.", "1.0.lock", "a@{b", "1:2"] {
            assert!(version_problem(version).is_some(), "{version}");
        }
    }

    #[test]
    fn writes_a_bcc_config_we_can_read_back_and_update() {
        let dir = std::env::temp_dir().join(format!("packraven-bcc-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pack = Pack { dir: dir.clone(), config: PackConfig::default(), lock: Lockfile::default() };
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(dir.join(BCC_CONFIG), "").unwrap();
        pack.write_bcc_config_if_missing("1.0.0").unwrap();
        assert_eq!(pack.version(), None, "we leave an existing config alone");

        std::fs::remove_file(dir.join(BCC_CONFIG)).unwrap();
        let pack = Pack { dir: dir.join("Raven's Fantasy"), ..pack };
        pack.write_bcc_config_if_missing("1.0.0").unwrap();
        assert_eq!(pack.name(), "Raven's Fantasy");
        assert_eq!(pack.version().as_deref(), Some("1.0.0"));
        pack.set_version("1.1.0").unwrap();
        assert_eq!(pack.version().as_deref(), Some("1.1.0"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
