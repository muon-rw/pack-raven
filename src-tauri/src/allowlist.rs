use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::http::CLIENT;
use crate::links::simplify;

// CurseForge keeps the mods it allows in packs without a CurseForge project in this public sheet, one
// tab per stretch of the alphabet.
const SHEET: &str = "https://docs.google.com/spreadsheets/d/176Wv-PZUo9hFxy6oC6N8tWdquBLPRtSuLbNK-r0_byM";
const RECHECK: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AllowedMod {
    pub name: String,
    pub author: String,
    pub license: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Allowlist {
    fetched_at: u64,
    mods: Vec<AllowedMod>,
}

impl Allowlist {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    /// Reads the sheet again once a day. When we can't, we keep the copy we have.
    pub async fn refresh(path: &Path) -> Result<Self> {
        let cached = Self::load(path);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        if now.saturating_sub(cached.fetched_at) < RECHECK.as_secs() {
            return Ok(cached);
        }
        let fresh = Self { fetched_at: now, mods: fetch().await.context("reading CurseForge's list of allowed mods")? };
        std::fs::write(path, serde_json::to_string(&fresh)?)?;
        Ok(fresh)
    }

    /// Returns the row for a mod, matched by its name and one of its authors, since many mods share a name.
    pub fn find(&self, name: &str, authors: &[String]) -> Option<&AllowedMod> {
        let name = simplify(name);
        let authors: Vec<String> = authors.iter().map(|a| simplify(a)).collect();
        self.mods.iter().find(|row| simplify(&row.name) == name && authors.contains(&simplify(&row.author)))
    }
}

async fn fetch() -> Result<Vec<AllowedMod>> {
    let page = CLIENT.get(format!("{SHEET}/htmlview")).send().await?.error_for_status()?.text().await?;
    let tabs = regex::Regex::new(r#"gid: "(\d+)""#).expect("valid regex");
    let mut gids: Vec<&str> = tabs.captures_iter(&page).filter_map(|c| c.get(1)).map(|m| m.as_str()).collect();
    gids.dedup();
    let mut mods = Vec::new();
    for gid in gids {
        let csv = CLIENT.get(format!("{SHEET}/export?format=csv&gid={gid}")).send().await?.error_for_status()?.text().await?;
        mods.extend(rows(&csv));
    }
    Ok(mods)
}

// Each tab starts with notes, and some tabs shift every column one to the right, so we find each row's
// columns from where its game is.
fn rows(csv: &str) -> Vec<AllowedMod> {
    parse_csv(csv)
        .into_iter()
        .filter_map(|cells| {
            let game = cells.iter().position(|c| c.trim().eq_ignore_ascii_case("minecraft"))?;
            let cell = |offset: usize| cells.get(game + offset).map(|c| c.trim().to_string()).unwrap_or_default();
            let (name, author) = (cell(1), cell(2));
            (!name.is_empty() && !author.is_empty()).then(|| AllowedMod { name, author, license: cell(3) })
        })
        .collect()
}

fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                chars.next();
                cell.push('"');
            }
            '"' => quoted = !quoted,
            ',' if !quoted => row.push(std::mem::take(&mut cell)),
            '\n' if !quoted => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            '\r' if !quoted => {}
            c => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_rows_from_every_tab_layout() {
        let csv = ",,,,\n\"If a mod is not listed here, you may NOT use it in a pack.\",,,,\nGame,Mod Name,Author,License,Notes\nMinecraft,ukulib,uku,Mozilla Public License Version 2.0,\nMinecraft,\"Navas, \"\"ZAmega\"\"\",natusue233,GPL,\r\n,Minecraft,Shifted,Someone,MIT\nMinecraft,,nobody,,";
        let names: Vec<(String, String)> = rows(csv).into_iter().map(|m| (m.name, m.author)).collect();
        assert_eq!(names, [("ukulib".into(), "uku".into()), ("Navas, \"ZAmega\"".into(), "natusue233".into()), ("Shifted".into(), "Someone".into())]);
    }

    #[test]
    fn matches_a_mod_by_name_and_author() {
        let list = Allowlist { fetched_at: 0, mods: vec![AllowedMod { name: "Kaleido Config".into(), author: "kennytv".into(), license: "MIT".into() }] };
        assert!(list.find("Kaleido config", &["KennyTV".into()]).is_some());
        assert!(list.find("Kaleido Config", &["someone-else".into()]).is_none(), "a mod with the same name by someone else isn't on the list");
    }
}
