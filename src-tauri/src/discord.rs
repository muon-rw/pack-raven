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
    pub project: Project,
    pub game: String,
}

pub struct Project {
    pub name: Option<String>,
    pub url: String,
    pub logo: Option<String>,
    pub author: Option<curseforge::CfAuthor>,
}

impl Announcement {
    fn fill(&self, template: &str) -> String {
        template
            .replace("{project_name}", self.project.name.as_deref().unwrap_or(&self.name))
            .replace("{name}", &self.name)
            .replace("{version}", &self.version)
            .replace("{file_url}", &self.file_url)
            .replace("{project_url}", &self.project.url)
            .trim()
            .to_string()
    }
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

fn shorten(text: &str, file_url: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let budget = limit.saturating_sub(120);
    let cut = text[..text.floor_char_boundary(budget)].rfind('\n').unwrap_or(text.floor_char_boundary(budget));
    format!("{}\n\n[Read the full changelog on CurseForge]({file_url})", text[..cut].trim_end())
}

fn color(hex: &str) -> u32 {
    u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0x4d6b2f)
}

pub fn message(announcement: &Announcement, config: &DiscordConfig) -> Value {
    let role = config.role_id.trim();
    let changelog = discord_markdown(announcement.changelog.trim());
    let header = announcement.fill(&config.header);
    let footer = announcement.fill(&config.footer);
    let body = if changelog.is_empty() {
        "A new version is out.".to_string()
    } else {
        shorten(&changelog, &announcement.file_url, DESCRIPTION_LIMIT.saturating_sub(header.len() + footer.len() + 4))
    };
    let description = [header, body, footer].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join("\n\n");
    let title = match announcement.fill(&config.title) {
        title if title.is_empty() => format!("{} {}", announcement.name, announcement.version),
        title => title,
    };
    let mut embed = json!({
        "title": title,
        "url": announcement.file_url,
        "description": description,
        "color": color(&config.color),
        "footer": { "text": announcement.game },
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });
    if let Some(logo) = &announcement.project.logo {
        embed["thumbnail"] = json!({ "url": logo });
    }
    if let Some(author) = announcement.project.author.as_ref().filter(|_| config.show_author) {
        embed["author"] = json!({ "name": author.name, "url": author.url, "icon_url": author.avatar_url });
    }
    json!({
        "username": announcement.name,
        "content": if role.is_empty() { String::new() } else { format!("<@&{role}>") },
        "allowed_mentions": { "parse": [], "roles": if role.is_empty() { vec![] } else { vec![role.to_string()] } },
        "embeds": [embed],
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

pub async fn is_approved(release: &Release) -> bool {
    matches!(curseforge::file(release.project_id, release.file_id).await, Ok(Some(file)) if APPROVED_STATUSES.contains(&file.file_status))
}

pub async fn announcement(pack: &Pack, release: &Release) -> Announcement {
    let changelog = std::fs::read_to_string(pack.dir.join(CHANGELOG_ARCHIVE).join(format!("{}.md", release.version))).unwrap_or_default();
    let project = project(release.project_id, &pack.config.author).await;
    Announcement {
        name: pack.name(),
        version: release.version.clone(),
        changelog,
        file_url: format!("{}/files/{}", project.url, release.file_id),
        project,
        game: game(pack),
    }
}

pub async fn project(project_id: u64, preferred_author: &str) -> Project {
    if project_id == 0 {
        return Project { name: None, url: "https://www.curseforge.com".into(), logo: None, author: None };
    }
    let found = curseforge::mods(&[project_id]).await.ok().and_then(|mut mods| mods.remove(&project_id));
    let authors = found.as_ref().map(|m| m.authors.clone()).unwrap_or_default();
    let author = authors.iter().find(|a| a.name.eq_ignore_ascii_case(preferred_author.trim())).or(authors.first()).cloned();
    Project {
        author,
        url: found
            .as_ref()
            .and_then(|m| m.links.website_url.as_deref())
            .map(|url| url.trim_end_matches('/').to_string())
            .unwrap_or_else(|| format!("https://www.curseforge.com/projects/{project_id}")),
        logo: found.as_ref().and_then(|m| m.logo.as_ref()).map(|logo| logo.thumbnail_url.clone()),
        name: found.map(|m| m.name),
    }
}

pub fn game(pack: &Pack) -> String {
    format!("Minecraft {}, {}", pack.lock.minecraft.as_deref().unwrap_or("?"), pack.lock.loader.as_deref().unwrap_or("?"))
}

pub async fn announce(pack_dir: &Path, releases_path: &Path, version: &str, webhook: &str) -> Result<()> {
    let pack = Pack::open(pack_dir)?;
    let mut releases = publish::load_releases(releases_path);
    let release = releases.iter_mut().find(|r| r.version == version).with_context(|| format!("you haven't published release {version} from this machine"))?;
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
        let short = shorten(&long, "https://example.com/files/1", DESCRIPTION_LIMIT);
        assert!(short.len() <= DESCRIPTION_LIMIT);
        assert!(short.ends_with("(https://example.com/files/1)"));
    }

    #[test]
    fn header_and_footer_fill_placeholders() {
        let announcement = Announcement {
            name: "Pack".into(),
            version: "1.0".into(),
            changelog: "- Foo".into(),
            file_url: "https://cf/files/7".into(),
            project: Project {
                name: Some("Pack CF".into()),
                url: "https://cf".into(),
                logo: Some("https://logo".into()),
                author: Some(curseforge::CfAuthor { name: "MuonR".into(), url: "https://cf/members/muonr".into(), avatar_url: Some("https://avatar".into()) }),
            },
            game: String::new(),
        };
        let config = DiscordConfig {
            header: "{name} {version}".into(),
            footer: "[Install]({file_url})".into(),
            title: "{project_name} - v{version}".into(),
            ..Default::default()
        };
        let embed = &message(&announcement, &config)["embeds"][0];
        assert_eq!(embed["title"], "Pack CF - v1.0");
        assert_eq!(embed["thumbnail"]["url"], "https://logo");
        assert_eq!(embed["author"]["icon_url"], "https://avatar");
        let description = embed["description"].as_str().unwrap().to_string();
        assert_eq!(description, "Pack 1.0\n\n- Foo\n\n[Install](https://cf/files/7)");
    }

    #[test]
    fn rejects_non_discord_urls() {
        assert!(validate_webhook("https://discord.com/api/webhooks/1/abc").is_ok());
        assert!(validate_webhook("https://example.com/hook").is_err());
    }
}
