use std::collections::BTreeMap;
use std::sync::Mutex;

use anyhow::{Context, Result};

use crate::credentials::KEYCHAIN_SERVICE;

// macOS asks for permission per keychain item and per read, so every secret lives in one item that we read once per run.
const BUNDLE_USER: &str = "secrets";
const LEGACY_USERS: [&str; 3] = ["curseforge-upload-token", "github-token", "microsoft-refresh-token"];

type Secrets = BTreeMap<String, String>;

static CACHE: Mutex<Option<Secrets>> = Mutex::new(None);

fn entry(user: &str) -> Result<keyring::Entry> {
    Ok(keyring::Entry::new(KEYCHAIN_SERVICE, user)?)
}

fn with_secrets<T>(f: impl FnOnce(&mut Secrets) -> T) -> Result<T> {
    let mut cache = CACHE.lock().unwrap();
    if cache.is_none() {
        *cache = Some(load()?);
    }
    Ok(f(cache.as_mut().unwrap()))
}

fn load() -> Result<Secrets> {
    match entry(BUNDLE_USER)?.get_password() {
        Ok(json) => Ok(serde_json::from_str(&json).unwrap_or_default()),
        Err(keyring::Error::NoEntry) => migrate_legacy(),
        Err(e) => Err(e).context("reading the system keychain"),
    }
}

fn migrate_legacy() -> Result<Secrets> {
    let mut secrets = Secrets::new();
    for user in LEGACY_USERS {
        if let Ok(secret) = entry(user)?.get_password() {
            secrets.insert(user.to_string(), secret);
        }
    }
    if !secrets.is_empty() {
        save(&secrets)?;
        for user in LEGACY_USERS {
            let _ = entry(user)?.delete_credential();
        }
    }
    Ok(secrets)
}

fn save(secrets: &Secrets) -> Result<()> {
    entry(BUNDLE_USER)?.set_password(&serde_json::to_string(secrets)?).context("saving to the system keychain")
}

pub fn get(key: &str) -> Option<String> {
    with_secrets(|s| s.get(key).cloned()).ok().flatten()
}

pub fn set(key: &str, secret: &str) -> Result<()> {
    update(|s| {
        s.insert(key.to_string(), secret.to_string());
    })
}

pub fn remove(key: &str) -> Result<()> {
    update(|s| {
        s.remove(key);
    })
}

// Commits to the cache only once the keychain write succeeds.
fn update(change: impl FnOnce(&mut Secrets)) -> Result<()> {
    with_secrets(|current| {
        let mut next = current.clone();
        change(&mut next);
        if next != *current {
            save(&next)?;
            *current = next;
        }
        Ok(())
    })?
}
