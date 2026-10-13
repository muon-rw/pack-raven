use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

// Matches `identifier` in tauri.conf.json, so the git hooks find the same config folder as the app.
const APP_IDENTIFIER: &str = "dev.muon.packraven";
const KEYCHAIN_SERVICE: &str = APP_IDENTIFIER;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Credential {
    Download,
    Upload,
}

impl Credential {
    fn keychain_user(self) -> &'static str {
        match self {
            Credential::Download => "curseforge-api-key",
            Credential::Upload => "curseforge-upload-token",
        }
    }

    // Earlier packraven versions only read these variables, so we keep using them until you choose a source.
    fn legacy_env_names(self) -> &'static [&'static str] {
        match self {
            Credential::Download => &["CF_DOWNLOAD_TOKEN"],
            Credential::Upload => &["CURSEFORGE_TOKEN", "CF_TOKEN"],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Source {
    #[default]
    None,
    Stored,
    Env {
        name: String,
    },
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Sources {
    download: Option<Source>,
    upload: Option<Source>,
}

impl Sources {
    fn slot(&mut self, credential: Credential) -> &mut Option<Source> {
        match credential {
            Credential::Download => &mut self.download,
            Credential::Upload => &mut self.upload,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialStatus {
    pub source: Source,
    pub present: bool,
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join(APP_IDENTIFIER)
}

fn sources_path() -> PathBuf {
    config_dir().join("credentials.json")
}

fn load_sources() -> Sources {
    std::fs::read_to_string(sources_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_source(credential: Credential, source: Source) -> Result<()> {
    let mut sources = load_sources();
    *sources.slot(credential) = Some(source);
    let path = sources_path();
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, serde_json::to_string_pretty(&sources)?).context("writing credentials.json")
}

fn source(credential: Credential) -> Source {
    load_sources().slot(credential).clone().unwrap_or_else(|| {
        credential
            .legacy_env_names()
            .iter()
            .find(|name| env_value(name).is_some())
            .map_or(Source::None, |name| Source::Env { name: name.to_string() })
    })
}

fn keychain(credential: Credential) -> Result<keyring::Entry> {
    Ok(keyring::Entry::new(KEYCHAIN_SERVICE, credential.keychain_user())?)
}

pub fn get(credential: Credential) -> Option<String> {
    let value = match source(credential) {
        Source::None => None,
        Source::Stored => keychain(credential).ok()?.get_password().ok(),
        Source::Env { name } => env_value(&name),
    };
    value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

pub fn status(credential: Credential) -> CredentialStatus {
    CredentialStatus { source: source(credential), present: get(credential).is_some() }
}

pub fn store(credential: Credential, secret: &str) -> Result<()> {
    let secret = secret.trim();
    if secret.is_empty() {
        bail!("Paste a key first.");
    }
    keychain(credential)?.set_password(secret).context("saving to the system keychain")?;
    save_source(credential, Source::Stored)
}

pub fn use_env(credential: Credential, name: &str) -> Result<()> {
    forget_stored(credential)?;
    save_source(credential, Source::Env { name: name.trim().to_string() })
}

pub fn clear(credential: Credential) -> Result<()> {
    forget_stored(credential)?;
    save_source(credential, Source::None)
}

fn forget_stored(credential: Credential) -> Result<()> {
    match keychain(credential)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e).context("removing the key from the system keychain"),
    }
}

pub fn keychain_name() -> &'static str {
    if cfg!(windows) { "Windows Credential Manager" } else { "the macOS Keychain" }
}

// Windows only hands new user variables to processes started after you set them, so we also read the registry.
fn env_value(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty()).or_else(|| user_env().into_iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v))
}

pub fn env_names() -> Vec<String> {
    let mut names: Vec<String> = std::env::vars_os().filter_map(|(name, _)| name.into_string().ok()).chain(user_env().into_iter().map(|(name, _)| name)).collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}

#[cfg(windows)]
fn user_env() -> Vec<(String, String)> {
    let Ok(key) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).open_subkey("Environment") else { return Vec::new() };
    key.enum_values().flatten().filter_map(|(name, _)| key.get_value::<String, _>(&name).ok().map(|value| (name, value))).collect()
}

#[cfg(not(windows))]
fn user_env() -> Vec<(String, String)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    #[test]
    fn keychain_persists_across_entries() {
        let entry = keyring::Entry::new("dev.muon.packraven.test", "roundtrip").unwrap();
        entry.set_password("secret-value").unwrap();
        let fresh = keyring::Entry::new("dev.muon.packraven.test", "roundtrip").unwrap();
        assert_eq!(fresh.get_password().unwrap(), "secret-value");
        fresh.delete_credential().unwrap();
        assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
    }
}
