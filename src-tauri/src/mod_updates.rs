use std::collections::{HashMap, HashSet};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::curseforge::{self, CfFile, CfFileIndex, CfMod};
use crate::modsync::{self, Placement};
use crate::pack::{LockEntry, ModLoader, Pack, ReleaseType};
use crate::sides::MetadataCache;

const DISABLED_SUFFIX: &str = ".disabled";
const MAX_DEPENDENCY_DEPTH: usize = 5;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledMod {
    path: String,
    project_id: u64,
    file_id: u64,
    name: String,
    url: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModFile {
    file_id: u64,
    file_name: String,
    release_type: ReleaseType,
    file_date: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModCheck {
    path: String,
    project_id: u64,
    logo: Option<String>,
    release_type: Option<ReleaseType>,
    requires: Vec<u64>,
    newer: Vec<ModFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModRequest {
    pub file_id: u64,
    pub replaces: Option<String>,
}

pub struct ModPlan {
    pub placements: Vec<Placement>,
    pub installs: Vec<String>,
    pub dependencies: Vec<String>,
    pub unavailable: Vec<String>,
}

pub fn release_type(curseforge: u8) -> ReleaseType {
    match curseforge {
        1 => ReleaseType::Release,
        2 => ReleaseType::Beta,
        _ => ReleaseType::Alpha,
    }
}

struct Target {
    minecraft: String,
    loaders: Vec<u32>,
}

impl Target {
    fn of(pack: &Pack) -> Result<Self> {
        let minecraft = pack.lock.minecraft()?.to_string();
        let loader = pack.lock.mod_loader()?;
        let mut loaders = vec![curseforge::loader_type(&loader)];
        // NeoForge 1.20.1 still loads Forge mods, and many authors only tag those Forge.
        if matches!(loader, ModLoader::NeoForge(_)) && minecraft == "1.20.1" {
            loaders.push(curseforge::loader_type(&ModLoader::Forge("")));
        }
        Ok(Self { minecraft, loaders })
    }

    // Resource packs and shaders have no loader, so an index without one fits any pack.
    fn fits(&self, index: &CfFileIndex) -> bool {
        index.game_version == self.minecraft && index.mod_loader.is_none_or(|l| self.loaders.contains(&l))
    }

    fn newest<'a>(&self, project: &'a CfMod, channel: ReleaseType) -> Option<&'a CfFileIndex> {
        project.latest_files_indexes.iter().filter(|i| self.fits(i) && release_type(i.release_type) <= channel).max_by_key(|i| i.file_id)
    }
}

pub async fn installed(pack: &Pack, cache: &mut MetadataCache) -> Vec<InstalledMod> {
    let ids: Vec<u64> = pack.lock.files.iter().map(|e| e.project_id).collect();
    // Offline, we still list every file, under its file name.
    let _ = cache.ensure_names(&ids).await;
    let mut mods: Vec<InstalledMod> = pack
        .lock
        .files
        .iter()
        .map(|entry| {
            let name = cache.known_name(entry.project_id);
            InstalledMod {
                path: entry.path.clone(),
                project_id: entry.project_id,
                file_id: entry.file_id,
                name: name.map_or_else(|| file_name(&entry.path).to_string(), |n| n.name.clone()),
                url: name.and_then(|n| n.url.clone()),
            }
        })
        .collect();
    mods.sort_by_key(|m| m.name.to_lowercase());
    mods
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub async fn check(pack: &Pack) -> Result<Vec<ModCheck>> {
    let target = Target::of(pack)?;
    let mut per_project: HashMap<u64, usize> = HashMap::new();
    for entry in &pack.lock.files {
        *per_project.entry(entry.project_id).or_default() += 1;
    }
    let project_ids: Vec<u64> = per_project.keys().copied().collect();
    let file_ids: Vec<u64> = pack.lock.files.iter().map(|e| e.file_id).collect();
    let projects = curseforge::mods(&project_ids).await?;
    let files = curseforge::files(&file_ids).await?;

    Ok(pack
        .lock
        .files
        .iter()
        .map(|entry| {
            let project = projects.get(&entry.project_id);
            let installed = files.get(&entry.file_id);
            // A pack can hold several files of one project, like resource pack variants, and we
            // can't tell which of them an update would replace.
            let newer = match project {
                Some(project) if per_project[&entry.project_id] == 1 => newer_files(&target, project, entry),
                _ => Vec::new(),
            };
            ModCheck {
                path: entry.path.clone(),
                project_id: entry.project_id,
                logo: project.and_then(|p| p.logo.as_ref()).map(|l| l.thumbnail_url.clone()),
                release_type: installed.map(|f| release_type(f.release_type)),
                requires: installed.map(|f| f.required_projects().collect()).unwrap_or_default(),
                newer,
            }
        })
        .collect())
}

fn newer_files(target: &Target, project: &CfMod, entry: &LockEntry) -> Vec<ModFile> {
    let mut newest: HashMap<u8, &CfFileIndex> = HashMap::new();
    for index in project.latest_files_indexes.iter().filter(|i| target.fits(i) && i.file_id > entry.file_id) {
        let slot = newest.entry(index.release_type).or_insert(index);
        if index.file_id > slot.file_id {
            *slot = index;
        }
    }
    let mut files: Vec<ModFile> = newest
        .into_values()
        .map(|i| ModFile { file_id: i.file_id, file_name: i.filename.clone(), release_type: release_type(i.release_type), file_date: None })
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.file_id));
    files
}

pub async fn versions(pack: &Pack, path: &str, project_id: u64) -> Result<Vec<ModFile>> {
    let target = Target::of(pack)?;
    let loader = path.starts_with("mods/").then_some(target.loaders[0]);
    let files = curseforge::project_files(project_id, &target.minecraft, loader).await?;
    Ok(files
        .into_iter()
        .map(|f| ModFile { file_id: f.id, release_type: release_type(f.release_type), file_date: Some(f.file_date), file_name: f.file_name })
        .collect())
}

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AddonKind {
    Mods,
    ResourcePacks,
    ShaderPacks,
    DataPacks,
}

// CurseForge's class IDs for Minecraft.
const KINDS: [(AddonKind, u64, &str); 4] =
    [(AddonKind::Mods, 6, "mods"), (AddonKind::ResourcePacks, 12, "resourcepacks"), (AddonKind::ShaderPacks, 6552, "shaderpacks"), (AddonKind::DataPacks, 6945, "datapacks")];

impl AddonKind {
    fn class_id(self) -> u64 {
        KINDS.iter().find(|(kind, ..)| *kind == self).map_or(6, |(_, id, _)| *id)
    }
}

fn folder_for(class_id: Option<u64>) -> &'static str {
    KINDS.iter().find(|(_, id, _)| Some(*id) == class_id).map_or("mods", |(.., folder)| folder)
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum SearchSort {
    Relevance,
    Popularity,
    Downloads,
    Newest,
    Updated,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    text: String,
    kind: AddonKind,
    category_id: Option<u64>,
    sort: SearchSort,
    index: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    project_id: u64,
    name: String,
    summary: String,
    authors: Vec<String>,
    downloads: u64,
    updated: String,
    logo: Option<String>,
    url: Option<String>,
    file: ModFile,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    results: Vec<SearchResult>,
    next_index: Option<u32>,
}

#[derive(Serialize)]
pub struct Category {
    id: u64,
    name: String,
}

const SEARCH_PAGE_SIZE: u32 = 20;
// CurseForge's largest page.
const RELEVANCE_POOL: u32 = 50;

pub async fn search(pack: &Pack, request: &SearchRequest) -> Result<SearchPage> {
    let target = Target::of(pack)?;
    // CurseForge's public API has no relevance sort, so we rank its most popular matches by how
    // closely their names match, and page through the rest by popularity.
    let rank = matches!(request.sort, SearchSort::Relevance) && request.index == 0 && !request.text.trim().is_empty();
    let page_size = if rank { RELEVANCE_POOL } else { SEARCH_PAGE_SIZE };
    let query = curseforge::SearchQuery {
        text: &request.text,
        class_id: request.kind.class_id(),
        category_id: request.category_id,
        minecraft: &target.minecraft,
        // Only mods have a loader. CurseForge returns no resource packs if we ask for one.
        loader_type: matches!(request.kind, AddonKind::Mods).then_some(target.loaders[0]),
        sort_field: match request.sort {
            SearchSort::Relevance | SearchSort::Popularity => 2,
            SearchSort::Updated => 3,
            SearchSort::Downloads => 6,
            SearchSort::Newest => 11,
        },
        index: request.index,
        page_size: page_size.min(curseforge::MAX_SEARCH_RESULTS.saturating_sub(request.index)),
    };
    let (mut projects, total) = curseforge::search(&query).await?;
    let next_index = request.index + projects.len() as u32;
    if rank {
        let text = request.text.trim();
        projects.sort_by_key(|p| name_match(&p.name, text));
    }
    let channel = pack.config.updates.channel;
    // CurseForge's version filter can match a project by a file for another loader, so we drop
    // projects we have no file to install from.
    let results = projects
        .into_iter()
        .filter_map(|project| {
            let file = target.newest(&project, channel).or_else(|| target.newest(&project, ReleaseType::Alpha)).map(|i| ModFile {
                file_id: i.file_id,
                file_name: i.filename.clone(),
                release_type: release_type(i.release_type),
                file_date: None,
            })?;
            Some(SearchResult {
                project_id: project.id,
                authors: project.authors.iter().map(|a| a.name.clone()).collect(),
                logo: project.logo.map(|l| l.thumbnail_url),
                url: project.links.website_url,
                name: project.name,
                summary: project.summary,
                downloads: project.download_count,
                updated: project.date_modified,
                file,
            })
        })
        .collect();
    Ok(SearchPage { results, next_index: (next_index < total).then_some(next_index) })
}

fn name_match(name: &str, text: &str) -> u8 {
    let simplify = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
    let (name, query) = (simplify(name), simplify(text));
    if name == query {
        0
    } else if name.starts_with(&query) {
        1
    } else if name.contains(&query) {
        2
    } else {
        3
    }
}

pub async fn categories(kind: AddonKind) -> Result<Vec<Category>> {
    Ok(curseforge::categories(kind.class_id()).await?.into_iter().map(|c| Category { id: c.id, name: c.name }).collect())
}

pub async fn plan(pack: &Pack, requests: &[ModRequest]) -> Result<ModPlan> {
    let target = Target::of(pack)?;
    let requested: Vec<u64> = requests.iter().map(|r| r.file_id).collect();
    let files = curseforge::files(&requested).await?;
    let file = |id: u64| files.get(&id).with_context(|| format!("CurseForge has no file {id}"));
    let new_projects: Vec<u64> = requests.iter().filter(|r| r.replaces.is_none()).map(|r| file(r.file_id).map(|f| f.mod_id)).collect::<Result<_>>()?;
    let new_projects = if new_projects.is_empty() { HashMap::new() } else { curseforge::mods(&new_projects).await? };
    let mut present: HashSet<u64> = pack.lock.files.iter().map(|e| e.project_id).collect();
    let mut placements = Vec::new();
    let mut installs = Vec::new();
    for request in requests {
        let file = file(request.file_id)?;
        let entry = match &request.replaces {
            Some(path) => {
                let (folder, _) = path.rsplit_once('/').with_context(|| format!("{path} isn't in a pack folder"))?;
                lock_entry(file, folder, path.ends_with(DISABLED_SUFFIX))?
            }
            None => {
                let project = new_projects.get(&file.mod_id).with_context(|| format!("CurseForge has no project {}", file.mod_id))?;
                if present.contains(&file.mod_id) {
                    bail!("{} is already in this pack", project.name);
                }
                installs.push(project.name.clone());
                lock_entry(file, folder_for(project.class_id), false)?
            }
        };
        placements.push(Placement { entry, replaces: request.replaces.clone() });
        present.insert(file.mod_id);
    }

    let mut dependencies = Vec::new();
    let mut unavailable = Vec::new();
    let mut wanted = missing_dependencies(files.values(), &present);
    for _ in 0..MAX_DEPENDENCY_DEPTH {
        if wanted.is_empty() {
            break;
        }
        present.extend(&wanted);
        let projects = curseforge::mods(&wanted).await?;
        let mut chosen = Vec::new();
        for id in &wanted {
            let Some(project) = projects.get(id) else {
                unavailable.push(format!("Project {id}"));
                continue;
            };
            // A missing dependency breaks the pack, so we take a less stable file over none.
            match target.newest(project, pack.config.updates.channel).or_else(|| target.newest(project, ReleaseType::Alpha)) {
                Some(index) => chosen.push((index.file_id, folder_for(project.class_id))),
                None => unavailable.push(project.name.clone()),
            }
        }
        let chosen_files = curseforge::files(&chosen.iter().map(|(id, _)| *id).collect::<Vec<_>>()).await?;
        for (file_id, folder) in chosen {
            let file = chosen_files.get(&file_id).with_context(|| format!("CurseForge has no file {file_id}"))?;
            placements.push(Placement { entry: lock_entry(file, folder, false)?, replaces: None });
            dependencies.push(projects[&file.mod_id].name.clone());
        }
        wanted = missing_dependencies(chosen_files.values(), &present);
    }
    Ok(ModPlan { placements, installs, dependencies, unavailable })
}

fn missing_dependencies<'a>(files: impl Iterator<Item = &'a CfFile>, present: &HashSet<u64>) -> Vec<u64> {
    let mut missing: Vec<u64> = files.flat_map(|f| f.required_projects()).filter(|id| !present.contains(id)).collect();
    missing.sort_unstable();
    missing.dedup();
    missing
}


pub(crate) fn lock_entry(file: &CfFile, folder: &str, disabled: bool) -> Result<LockEntry> {
    if file.file_name.contains(['/', '\\']) || file.file_name.starts_with('.') {
        bail!("CurseForge returned an unsafe file name: {}", file.file_name);
    }
    let sha1 = file.sha1().with_context(|| format!("CurseForge has no SHA-1 for {}", file.file_name))?;
    let suffix = if disabled { DISABLED_SUFFIX } else { "" };
    Ok(LockEntry {
        path: format!("{folder}/{}{suffix}", file.file_name),
        project_id: file.mod_id,
        file_id: file.id,
        sha1: sha1.to_string(),
        size: file.file_length,
        url: file.download_url.clone().unwrap_or_else(|| modsync::cdn_url(file.id, &file.file_name)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(file_id: u64, release_type: u8, mod_loader: Option<u32>) -> CfFileIndex {
        CfFileIndex { game_version: "1.21.1".into(), file_id, filename: format!("f{file_id}.jar"), release_type, mod_loader }
    }

    fn project(indexes: Vec<CfFileIndex>) -> CfMod {
        serde_json::from_value(serde_json::json!({ "id": 1, "name": "A", "slug": "a" })).map(|m: CfMod| CfMod { latest_files_indexes: indexes, ..m }).unwrap()
    }

    fn target() -> Target {
        Target { minecraft: "1.21.1".into(), loaders: vec![6] }
    }

    fn entry(file_id: u64) -> LockEntry {
        LockEntry { path: "mods/a.jar".into(), project_id: 1, file_id, sha1: String::new(), size: 0, url: String::new() }
    }

    #[test]
    fn offers_the_newest_file_of_each_release_type() {
        let fabric = index(900, 1, Some(4));
        let other_version = CfFileIndex { game_version: "1.20.1".into(), ..index(950, 1, Some(6)) };
        let project = project(vec![index(500, 1, Some(6)), index(400, 2, Some(6)), index(300, 3, Some(6)), fabric, other_version]);
        let newer: Vec<(u64, ReleaseType)> = newer_files(&target(), &project, &entry(350)).iter().map(|f| (f.file_id, f.release_type)).collect();
        assert_eq!(newer, [(500, ReleaseType::Release), (400, ReleaseType::Beta)]);
    }

    #[test]
    fn newest_respects_the_channel() {
        let project = project(vec![index(500, 1, Some(6)), index(600, 2, Some(6)), index(700, 3, None)]);
        assert_eq!(target().newest(&project, ReleaseType::Release).map(|i| i.file_id), Some(500));
        assert_eq!(target().newest(&project, ReleaseType::Beta).map(|i| i.file_id), Some(600));
        assert_eq!(target().newest(&project, ReleaseType::Alpha).map(|i| i.file_id), Some(700));
    }

    #[test]
    fn ranks_names_by_how_closely_they_match() {
        let mut names = vec!["Sophisticated Backpacks Create Integration", "Backpacked", "Sophisticated Backpacks", "Create: Sophisticated Backpacks Compat"];
        names.sort_by_key(|n| name_match(n, "sophisticated backpacks"));
        assert_eq!(names, ["Sophisticated Backpacks", "Sophisticated Backpacks Create Integration", "Create: Sophisticated Backpacks Compat", "Backpacked"]);
    }

    #[test]
    fn keeps_disabled_files_disabled() {
        let file: CfFile = serde_json::from_value(serde_json::json!({
            "id": 5, "modId": 1, "displayName": "A", "fileName": "a-2.jar",
            "hashes": [{ "value": "md5", "algo": 2 }, { "value": "abc", "algo": 1 }]
        }))
        .unwrap();
        let entry = lock_entry(&file, "mods", true).unwrap();
        assert_eq!(entry.path, "mods/a-2.jar.disabled");
        assert_eq!(entry.sha1, "abc");
        assert_eq!(entry.url, "https://edge.forgecdn.net/files/0/5/a-2.jar");
    }
}
