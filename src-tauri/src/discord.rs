use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use crate::curseforge;
use crate::http::CLIENT;
use crate::pack::{DiscordConfig, Pack, CHANGELOG_ARCHIVE};
use crate::publish::{self, Release};

const DESCRIPTION_LIMIT: usize = 4000;
const DISCOHOOK_SHARE: &str = "https://discohook.app/api/v1/share";
const APPROVED_STATUSES: [u32; 2] = [4, 10];

pub struct Announcement {
    pub name: String,
    pub version: String,
    pub changelog: String,
    pub file_url: String,
    pub game: String,
}

pub fn validate_webhook(url: &str) -> Result<()> {
    let valid = ["https://discord.com/api/webhooks/", "https://discordapp.com/api/webhooks/", "https://ptb.discord.com/api/webhooks/", "https://canary.discord.com/api/webhooks/"]
        .iter()
        .any(|prefix| url.starts_with(prefix));
    if !valid {
        bail!("That isn't a Discord webhook URL. Copy it from Server Settings, Integrations, Webhooks.");
    }
    Ok(())
}

fn discord_markdown(changelog: &str) -> String {
    changelog
        .lines()
        .map(|line| match line.trim_start().split_once(' ') {
            Some((hashes, title)) if !hashes.is_empty() && hashes.chars().all(|c| c == '#') => format!("**{title}**"),
            _ => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn shorten(text: &str, file_url: &str) -> String {
    if text.len() <= DESCRIPTION_LIMIT {
        return text.to_string();
    }
    let cut = text[..text.floor_char_boundary(DESCRIPTION_LIMIT - 120)].rfind('\n').unwrap_or(DESCRIPTION_LIMIT - 120);
    format!("{}\n\n[Read the full changelog on CurseForge]({file_url})", text[..cut].trim_end())
}

fn color(hex: &str) -> u32 {
    u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0x4d6b2f)
}

pub fn message(announcement: &Announcement, config: &DiscordConfig) -> Value {
    let role = config.role_id.trim();
    let changelog = discord_markdown(announcement.changelog.trim());
    let description = if changelog.is_empty() { "A new version is out.".to_string() } else { shorten(&changelog, &announcement.file_url) };
    json!({
        "username": announcement.name,
        "content": if role.is_empty() { String::new() } else { format!("<@&{role}>") },
        "allowed_mentions": { "parse": [], "roles": if role.is_empty() { vec![] } else { vec![role.to_string()] } },
        "embeds": [{
            "title": format!("{} {}", announcement.name, announcement.version),
            "url": announcement.file_url,
            "description": description,
            "color": color(&config.color),
            "footer": { "text": announcement.game },
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }],
    })
}

pub async fn send(webhook: &str, message: &Value) -> Result<String> {
    validate_webhook(webhook)?;
    let response = CLIENT.post(format!("{webhook}?wait=true")).json(message).send().await?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        bail!("Discord rejected the message ({status}): {}", body.get("message").and_then(Value::as_str).unwrap_or("no details"));
    }
    Ok(body.get("id").and_then(Value::as_str).unwrap_or_default().to_string())
}

pub async fn discohook_link(message: &Value) -> Result<String> {
    let mut data = message.clone();
    if let Some(object) = data.as_object_mut() {
        object.remove("allowed_mentions");
    }
    let body = json!({ "data": { "version": "d2", "messages": [{ "data": data }] } });
    let response: Value = CLIENT.post(DISCOHOOK_SHARE).json(&body).send().await?.error_for_status()?.json().await?;
    response.get("url").and_then(Value::as_str).map(String::from).context("Discohook didn't return a link")
}

pub async fn file_url(project_id: u64, file_id: u64) -> String {
    let website = curseforge::mods(&[project_id]).await.ok().and_then(|mods| mods.get(&project_id).and_then(|m| m.links.website_url.clone()));
    match website {
        Some(url) => format!("{}/files/{file_id}", url.trim_end_matches('/')),
        None => format!("https://www.curseforge.com/projects/{project_id}"),
    }
}

pub async fn is_approved(release: &Release) -> bool {
    matches!(curseforge::file(release.project_id, release.file_id).await, Ok(Some(file)) if APPROVED_STATUSES.contains(&file.file_status))
}

pub async fn announcement(pack: &Pack, release: &Release) -> Announcement {
    let changelog = std::fs::read_to_string(pack.dir.join(CHANGELOG_ARCHIVE).join(format!("{}.md", release.version))).unwrap_or_default();
    Announcement {
        name: pack.name(),
        version: release.version.clone(),
        changelog,
        file_url: file_url(release.project_id, release.file_id).await,
        game: format!("Minecraft {}, {}", pack.lock.minecraft.as_deref().unwrap_or("?"), pack.lock.loader.as_deref().unwrap_or("?")),
    }
}

pub async fn announce(pack_dir: &Path, releases_path: &Path, version: &str, webhook: &str) -> Result<()> {
    let pack = Pack::open(pack_dir)?;
    let mut releases = publish::load_releases(releases_path);
    let release = releases.iter_mut().find(|r| r.version == version).with_context(|| format!("no release {version} was published from this machine"))?;
    let message = message(&announcement(&pack, release).await, &pack.config.discord);
    send(webhook, &message).await?;
    release.announced_at = Some(chrono::Utc::now().to_rfc3339());
    publish::save_releases(releases_path, &releases)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_become_bold_and_long_text_links_out() {
        assert_eq!(discord_markdown("## Mod changes\n### Added\n- Foo"), "**Mod changes**\n**Added**\n- Foo");
        let long = "- line\n".repeat(1000);
        let short = shorten(&long, "https://example.com/files/1");
        assert!(short.len() <= DESCRIPTION_LIMIT);
        assert!(short.ends_with("(https://example.com/files/1)"));
    }

    #[test]
    fn rejects_non_discord_urls() {
        assert!(validate_webhook("https://discord.com/api/webhooks/1/abc").is_ok());
        assert!(validate_webhook("https://example.com/hook").is_err());
    }
}
