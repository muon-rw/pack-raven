use std::collections::HashSet;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::curseforge::{self, CfFile, CfMod};
use crate::modrinth::{self, Project, Version};
use crate::mod_updates::release_type;
use crate::pack::{CurseforgeFile, ReleaseType};
use crate::twins::{self, Target};

const SEARCH_RESULTS: u32 = 3;

/// What convinced us a Modrinth project is the same mod as a CurseForge one. Two different mods can
/// share a name, so a name alone never links them.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Evidence {
    #[serde(rename = "sameFiles")]
    FileNames,
    #[serde(rename = "sameSource")]
    Repository,
    #[serde(rename = "sameAuthor")]
    Author,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModrinthLink {
    pub project_id: String,
    pub title: String,
    pub url: String,
    pub evidence: Evidence,
}

/// A version both platforms have, as CurseForge's file, which is the one we'd put on disk.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SharedVersion {
    pub file_id: u64,
    pub name: String,
    pub newer: bool,
}

/// What Modrinth has of a mod when it doesn't host the pack's file of it.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Skew {
    pub link: ModrinthLink,
    /// The newest version both platforms have for the pack's Minecraft and loader.
    pub shared: Option<SharedVersion>,
    /// Modrinth's newest version for the pack's Minecraft and loader.
    pub modrinth_newest: Option<String>,
    pub modrinth_newer: bool,
    /// CurseForge has Modrinth's newest version too.
    pub newest_on_curseforge: bool,
    /// Modrinth has a file named like the pack's, with different contents.
    pub same_name_differs: bool,
}

/// Looks for the Modrinth project of a CurseForge mod whose file Modrinth doesn't host, and what
/// versions of it each platform has.
pub async fn skew(target: &Target, loader_type: Option<u32>, channel: ReleaseType, installed: &CurseforgeFile, project: &CfMod) -> Result<Option<Skew>> {
    let curseforge_files = curseforge::project_files(project.id, &target.minecraft, loader_type).await?;
    let loaders: &[&str] = if loader_type.is_some() { &target.mod_loaders } else { &[] };
    let Some((found, versions, evidence)) = linked_project(target, loaders, project, &curseforge_files).await? else { return Ok(None) };

    let modrinth_names: HashSet<String> = versions.iter().flat_map(|v| &v.files).map(|f| f.filename.to_lowercase()).collect();
    let on_both = |file: &&CfFile| modrinth_names.contains(&file.file_name.to_lowercase());
    let same_name_differs = curseforge_files.iter().filter(on_both).any(|f| f.id == installed.file_id);
    let shared_files: Vec<&CfFile> = curseforge_files.iter().filter(on_both).filter(|f| f.id != installed.file_id).collect();
    let in_channel = shared_files.iter().filter(|f| release_type(f.release_type) <= channel).max_by_key(|f| f.id);
    let shared = in_channel.or_else(|| shared_files.iter().max_by_key(|f| f.id));

    let installed_date = curseforge_files.iter().find(|f| f.id == installed.file_id).map(|f| f.file_date.clone());
    let installed_date = match installed_date {
        Some(date) => Some(date),
        None => curseforge::files(&[installed.file_id]).await?.remove(&installed.file_id).map(|f| f.file_date),
    };
    let modrinth_newest = versions.iter().max_by(|a, b| a.date_published.cmp(&b.date_published));
    // Authors often upload to Modrinth after CurseForge, so when CurseForge has the same file we go by
    // CurseForge's order instead of the upload dates.
    let on_curseforge = |version: &Version| version.files.iter().find_map(|f| curseforge_files.iter().find(|c| c.file_name.eq_ignore_ascii_case(&f.filename))).map(|c| c.id);
    let newest_on_curseforge = modrinth_newest.and_then(on_curseforge);
    let modrinth_newer = match (modrinth_newest, newest_on_curseforge) {
        (_, Some(file_id)) => file_id > installed.file_id,
        (Some(version), None) => installed_date.is_some_and(|date| is_later(&version.date_published, &date)),
        (None, None) => false,
    };
    Ok(Some(Skew {
        link: ModrinthLink { url: found.url(), project_id: found.id, title: found.title, evidence },
        shared: shared.map(|f| SharedVersion { file_id: f.id, name: f.display_name.clone(), newer: f.id > installed.file_id }),
        modrinth_newer,
        newest_on_curseforge: newest_on_curseforge.is_some(),
        modrinth_newest: modrinth_newest.map(|v| if v.version_number.is_empty() { v.name.clone() } else { v.version_number.clone() }),
        same_name_differs,
    }))
}

// CurseForge and Modrinth both write ISO 8601 times, with different fractions of a second.
fn is_later(a: &str, b: &str) -> bool {
    a.get(..19).unwrap_or(a) > b.get(..19).unwrap_or(b)
}

// We look for the project the way the twin search does, by slug and then by name, and keep the first
// one with evidence. Matching files are the strongest evidence, then a shared source repository.
async fn linked_project(target: &Target, loaders: &[&str], project: &CfMod, curseforge_files: &[CfFile]) -> Result<Option<(Project, Vec<Version>, Evidence)>> {
    let curseforge_names: HashSet<String> = curseforge_files.iter().map(|f| f.file_name.to_lowercase()).collect();
    let mut tried: Vec<String> = Vec::new();
    let mut by_slug = Vec::new();
    for slug in std::iter::once(project.slug.as_str()).chain(twins::slug_for_every_loader(&project.slug)) {
        if let Some(found) = modrinth::project(slug).await? {
            by_slug.push(found);
        }
    }
    let mut candidates = by_slug.into_iter();
    let mut searched = false;
    loop {
        let found = match candidates.next() {
            Some(found) => found,
            None if !searched => {
                searched = true;
                let hits = modrinth::search(&twins::name_for_every_loader(&project.name), &target.minecraft, SEARCH_RESULTS).await?;
                let ids: Vec<String> = hits.into_iter().map(|h| h.project_id).filter(|id| !tried.contains(id)).collect();
                let mut found = modrinth::projects(&ids).await?;
                candidates = ids.iter().filter_map(|id| found.remove(id)).collect::<Vec<_>>().into_iter();
                continue;
            }
            None => return Ok(None),
        };
        if tried.contains(&found.id) {
            continue;
        }
        tried.push(found.id.clone());
        let versions = modrinth::project_versions(&found.id, loaders, &target.minecraft).await?;
        if let Some(evidence) = evidence(project, &found, &versions, &curseforge_names).await? {
            return Ok(Some((found, versions, evidence)));
        }
    }
}

async fn evidence(project: &CfMod, found: &Project, versions: &[Version], curseforge_names: &HashSet<String>) -> Result<Option<Evidence>> {
    if versions.iter().flat_map(|v| &v.files).any(|f| curseforge_names.contains(&f.filename.to_lowercase())) {
        return Ok(Some(Evidence::FileNames));
    }
    if project.links.source_url.as_deref().and_then(repository).is_some_and(|ours| found.source_url.as_deref().and_then(repository) == Some(ours)) {
        return Ok(Some(Evidence::Repository));
    }
    let names: HashSet<String> = project.authors.iter().map(|a| simplify(&a.name)).collect();
    let theirs = modrinth::authors(&[found]).await?.remove(&found.id).unwrap_or_default();
    Ok(theirs.iter().any(|name| names.contains(&simplify(name))).then_some(Evidence::Author))
}

/// Returns the Modrinth page of each CurseForge project that Modrinth has too, by CurseForge project ID.
/// Browsing shows these for a page of results at a time, so we only ask Modrinth by slug, in one batch,
/// and only trust a project with the same source repository or author.
pub async fn modrinth_counterparts(ids: &[u64]) -> Result<Vec<(u64, String)>> {
    let projects = curseforge::mods(ids).await?;
    let slugs: Vec<String> = projects.values().flat_map(|p| std::iter::once(p.slug.as_str()).chain(twins::slug_for_every_loader(&p.slug))).map(str::to_lowercase).collect();
    let found = modrinth::projects(&slugs).await?;
    let by_slug: std::collections::HashMap<String, &Project> = found.values().map(|p| (p.slug.to_lowercase(), p)).collect();
    let candidates: Vec<(&CfMod, &Project)> = projects
        .values()
        .filter_map(|cf| {
            let slug = cf.slug.to_lowercase();
            let modrinth = by_slug.get(&slug).or_else(|| twins::slug_for_every_loader(&slug).and_then(|s| by_slug.get(s)))?;
            Some((cf, *modrinth))
        })
        .collect();
    let authors = modrinth::authors(&candidates.iter().map(|(_, m)| *m).collect::<Vec<_>>()).await?;
    Ok(candidates
        .into_iter()
        .filter(|(cf, modrinth)| {
            let same_source = cf.links.source_url.as_deref().and_then(repository).is_some_and(|ours| modrinth.source_url.as_deref().and_then(repository) == Some(ours));
            let names: HashSet<String> = cf.authors.iter().map(|a| simplify(&a.name)).collect();
            same_source || authors.get(&modrinth.id).into_iter().flatten().any(|name| names.contains(&simplify(name)))
        })
        .map(|(cf, modrinth)| (cf.id, modrinth.url()))
        .collect())
}

/// Returns the CurseForge page of each Modrinth project CurseForge has too, by Modrinth project ID.
/// CurseForge can only look up one slug at a time, so we ask about a page of results, not more.
pub async fn curseforge_counterparts(ids: &[String]) -> Result<Vec<(String, String)>> {
    let projects = modrinth::projects(ids).await?;
    let list: Vec<&Project> = projects.values().collect();
    let authors = modrinth::authors(&list).await?;
    let mut found = Vec::new();
    for project in list {
        let theirs: HashSet<String> = authors.get(&project.id).into_iter().flatten().map(|name| simplify(name)).collect();
        let ours = project.source_url.as_deref().and_then(repository);
        for candidate in curseforge::mods_with_slug(&project.slug).await? {
            let same_source = ours.is_some() && candidate.links.source_url.as_deref().and_then(repository) == ours;
            if same_source || candidate.authors.iter().any(|a| theirs.contains(&simplify(&a.name))) {
                if let Some(url) = candidate.links.website_url {
                    found.push((project.id.clone(), url));
                }
                break;
            }
        }
    }
    Ok(found)
}

// Links to one repository take many forms, like github.com/Owner/Repo.git or .../repo/tree/main.
fn repository(url: &str) -> Option<String> {
    let url = url.trim().to_lowercase();
    let rest = url.split_once("://").map_or(url.as_str(), |(_, rest)| rest);
    let mut parts = rest.trim_start_matches("www.").split('/').filter(|p| !p.is_empty());
    let (host, owner, repo) = (parts.next()?, parts.next()?, parts.next()?);
    Some(format!("{host}/{owner}/{}", repo.trim_end_matches(".git")))
}

pub fn simplify(name: &str) -> String {
    name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_one_repository_from_its_many_links() {
        let expected = Some("github.com/caffeinemc/sodium".to_string());
        for url in ["https://github.com/CaffeineMC/sodium", "https://www.github.com/CaffeineMC/sodium.git", "http://github.com/CaffeineMC/sodium/tree/dev/", "github.com/caffeinemc/sodium"] {
            assert_eq!(repository(url), expected, "{url}");
        }
        assert_eq!(repository("https://github.com/CaffeineMC"), None, "an owner isn't a repository");
    }

    #[test]
    fn compares_times_from_both_platforms() {
        assert!(is_later("2026-08-28T20:55:02.593669Z", "2026-08-28T20:55:01.12Z"));
        assert!(!is_later("2025-01-01T00:00:00Z", "2026-08-28T20:55:01.12Z"));
    }
}
