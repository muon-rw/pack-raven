use std::path::Path;

use anyhow::Result;

use crate::curseforge::{self, CfMod};
use crate::http;
use crate::modrinth;
use crate::pack::{LockEntry, Lockfile, ModLoader, ModrinthFile};

const SEARCH_RESULTS: u32 = 3;

pub struct Target {
    minecraft: String,
    mod_loaders: Vec<&'static str>,
}

impl Target {
    pub fn of(lock: &Lockfile) -> Option<Self> {
        let minecraft = lock.minecraft.clone()?;
        let mut mod_loaders = vec![match lock.mod_loader().ok()? {
            ModLoader::NeoForge(_) => "neoforge",
            ModLoader::Forge(_) => "forge",
            ModLoader::Fabric(_) => "fabric",
        }];
        // NeoForge 1.20.1 still loads Forge mods, and many authors only tag those Forge.
        if mod_loaders[0] == "neoforge" && minecraft == "1.20.1" {
            mod_loaders.push("forge");
        }
        Some(Self { minecraft, mod_loaders })
    }
}

/// Finds Modrinth's copy of the CurseForge file behind each entry, for files Modrinth doesn't host
/// byte for byte.
pub async fn find(root: &Path, cache_dir: &Path, target: &Target, entries: &mut [&mut LockEntry]) -> Result<()> {
    let ids: Vec<u64> = entries.iter().filter_map(|e| e.curseforge_project()).collect();
    if ids.is_empty() || curseforge::core_key().is_none() {
        return Ok(());
    }
    let projects = curseforge::mods(&ids).await?;
    // Modrinth allows 300 requests a minute, so we ask about one entry at a time.
    for entry in entries.iter_mut() {
        let Some(project) = entry.curseforge_project().and_then(|id| projects.get(&id)) else { continue };
        if let Some(twin) = twin(root, cache_dir, target, entry, project).await? {
            entry.modrinth = Some(twin);
        }
    }
    Ok(())
}

// Most authors use the same project slug on both platforms, so we only search by name when that misses.
async fn twin(root: &Path, cache_dir: &Path, target: &Target, entry: &LockEntry, project: &CfMod) -> Result<Option<ModrinthFile>> {
    let mut tried = Vec::new();
    for slug in std::iter::once(project.slug.as_str()).chain(slug_for_every_loader(&project.slug)) {
        let Some(found) = modrinth::project(slug).await? else { continue };
        if tried.contains(&found.id) {
            continue;
        }
        if let Some(twin) = twin_in(root, cache_dir, target, entry, &found.id).await? {
            return Ok(Some(twin));
        }
        tried.push(found.id);
    }
    for hit in modrinth::search(&name_for_every_loader(&project.name), &target.minecraft, SEARCH_RESULTS).await? {
        if tried.contains(&hit.project_id) {
            continue;
        }
        if let Some(twin) = twin_in(root, cache_dir, target, entry, &hit.project_id).await? {
            return Ok(Some(twin));
        }
        tried.push(hit.project_id);
    }
    Ok(None)
}

// Some authors split a mod into one CurseForge project per loader, like terrablender-neoforge or
// "TerraBlender (NeoForge)", where Modrinth has one project for every loader.
fn slug_for_every_loader(slug: &str) -> Option<&str> {
    ["-neoforge", "-forge", "-fabric", "-quilt"].iter().find_map(|suffix| slug.strip_suffix(suffix))
}

fn name_for_every_loader(name: &str) -> String {
    static LOADER_NOTE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"\s*[(\[][^)\]]*[)\]]\s*$").expect("valid regex"));
    let mut general = name.to_string();
    while LOADER_NOTE.is_match(&general) {
        general = LOADER_NOTE.replace(&general, "").into_owned();
    }
    if general.is_empty() { name.to_string() } else { general }
}

// We only download a candidate whose file name matches the jar's, since publishing tools upload the
// same build under the same name to both platforms.
async fn twin_in(root: &Path, cache_dir: &Path, target: &Target, entry: &LockEntry, project_id: &str) -> Result<Option<ModrinthFile>> {
    let name = entry.path.rsplit('/').next().unwrap_or(&entry.path).trim_end_matches(".disabled");
    let loaders: &[&str] = if entry.path.starts_with("mods/") { &target.mod_loaders } else { &[] };
    for version in modrinth::project_versions(project_id, loaders, &target.minecraft).await? {
        for file in version.files.iter().filter(|f| f.filename.eq_ignore_ascii_case(name)) {
            let cached = cache_dir.join(&file.hashes.sha1);
            if !cached.exists() {
                http::download(&file.url, &cached, Some(&file.hashes.sha1)).await?;
            }
            if same_contents(&root.join(&entry.path), &cached) {
                return Ok(Some(version.lock_file(file)));
            }
        }
    }
    Ok(None)
}

// Gradle's publishing tasks can stamp each upload with its own timestamps and manifest attributes,
// so the same build can hash differently on each platform. We compare what the jars hold instead,
// ignoring the manifest and the signature files that cover it.
pub fn same_contents(a: &Path, b: &Path) -> bool {
    match (listing(a), listing(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn listing(path: &Path) -> Result<Vec<(String, u32, u64)>> {
    let mut entries = Vec::new();
    list_into(&mut zip::ZipArchive::new(std::fs::File::open(path)?)?, "", 0, &mut entries)?;
    entries.sort();
    Ok(entries)
}

// Fabric's META-INF/jars and NeoForge's META-INF/jarjar hold libraries the build re-zips for each
// upload, so we compare what those hold too.
const NESTED_JAR_DEPTH: usize = 3;

fn list_into<R: std::io::Read + std::io::Seek>(archive: &mut zip::ZipArchive<R>, prefix: &str, depth: usize, entries: &mut Vec<(String, u32, u64)>) -> Result<()> {
    for index in 0..archive.len() {
        let (name, crc, size, is_dir) = {
            let file = archive.by_index_raw(index)?;
            (file.name()?.into_owned(), file.crc32(), file.size(), file.is_dir())
        };
        if is_dir || is_manifest_or_signature(&name) {
            continue;
        }
        if name.ends_with(".jar") && depth < NESTED_JAR_DEPTH {
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut archive.by_index(index)?, &mut bytes)?;
            if let Ok(mut nested) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) {
                list_into(&mut nested, &format!("{prefix}{name}!/"), depth + 1, entries)?;
                continue;
            }
        }
        if is_generated_list(&name) {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut archive.by_index(index)?, &mut text)?;
            entries.push((format!("{prefix}{name}"), digest_of(&sorted_lines(&text)), size));
            continue;
        }
        // The injected class names its own package, so its bytes differ along with its name.
        match without_build_id(&name) {
            Some(general) => entries.push((format!("{prefix}{general}"), 0, 0)),
            None => entries.push((format!("{prefix}{name}"), crc, size)),
        }
    }
    Ok(())
}

// Builds write access transformers and wideners from a set, in whatever order it iterates, so two
// builds of one source can list the same lines in a different order.
fn is_generated_list(name: &str) -> bool {
    name.ends_with("accesstransformer.cfg") || name.ends_with(".accesswidener")
}

fn digest_of(text: &str) -> u32 {
    use sha1::Digest;
    let digest = sha1::Sha1::digest(text.as_bytes());
    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
}

fn sorted_lines(text: &str) -> String {
    let mut lines: Vec<&str> = text.lines().map(str::trim_end).filter(|line| !line.is_empty()).collect();
    lines.sort_unstable();
    lines.join("\n")
}

// Architectury Loom injects a class into a package named with a random ID that changes every build,
// so two uploads of one build differ there and nowhere else.
fn without_build_id(name: &str) -> Option<String> {
    static BUILD_ID: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"^(architectury_inject_\w+?_)[0-9a-f]{32}_").expect("valid regex"));
    BUILD_ID.is_match(name).then(|| BUILD_ID.replace(name, "${1}_").into_owned())
}

fn is_manifest_or_signature(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper == "META-INF/MANIFEST.MF" || (upper.starts_with("META-INF/") && [".SF", ".RSA", ".DSA", ".EC"].iter().any(|e| upper.ends_with(e)))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn jar(path: &Path, entries: &[(&str, &[u8])], manifest: &str) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        zip.start_file("META-INF/MANIFEST.MF", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(manifest.as_bytes()).unwrap();
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    fn nested(class: &[u8], year: u16) -> Vec<u8> {
        let options = zip::write::SimpleFileOptions::default().last_modified_time(zip::DateTime::from_date_and_time(year, 1, 1, 0, 0, 0).unwrap());
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file("lib/C.class", options).unwrap();
        zip.write_all(class).unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn compares_nested_jars_by_what_they_hold() {
        let dir = std::env::temp_dir().join(format!("packraven-nested-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b, c) = (dir.join("a.jar"), dir.join("b.jar"), dir.join("c.jar"));
        let (old, new, changed) = (nested(b"lib", 2020), nested(b"lib", 2024), nested(b"patched", 2020));
        assert_ne!(old, new, "the nested jars' bytes differ");
        jar(&a, &[("META-INF/jars/lib.jar", &old), ("A.class", b"a")], "");
        jar(&b, &[("META-INF/jars/lib.jar", &new), ("A.class", b"a")], "");
        jar(&c, &[("META-INF/jars/lib.jar", &changed), ("A.class", b"a")], "");
        assert!(same_contents(&a, &b));
        assert!(!same_contents(&a, &c));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reordered_access_transformers_are_the_same() {
        let dir = std::env::temp_dir().join(format!("packraven-at-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b, c) = (dir.join("a.jar"), dir.join("b.jar"), dir.join("c.jar"));
        jar(&a, &[("META-INF/accesstransformer.cfg", b"public a.B c\npublic d.E f\n")], "");
        jar(&b, &[("META-INF/accesstransformer.cfg", b"public d.E f\npublic a.B c\n")], "");
        jar(&c, &[("META-INF/accesstransformer.cfg", b"public d.E f\npublic a.B g\n")], "");
        assert!(same_contents(&a, &b));
        assert!(!same_contents(&a, &c));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn looks_past_curseforge_projects_split_by_loader() {
        assert_eq!(slug_for_every_loader("terrablender-neoforge"), Some("terrablender"));
        assert_eq!(slug_for_every_loader("jei"), None);
        assert_eq!(name_for_every_loader("TerraBlender (NeoForge)"), "TerraBlender");
        assert_eq!(name_for_every_loader("Paxi (NeoForge) [1.20.4 - 1.21.1 ONLY]"), "Paxi");
        assert_eq!(name_for_every_loader("[Mod] (x)"), "[Mod] (x)");
    }

    #[test]
    fn jars_with_the_same_classes_are_twins_whatever_their_manifests_say() {
        let dir = std::env::temp_dir().join(format!("packraven-twins-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b, c) = (dir.join("a.jar"), dir.join("b.jar"), dir.join("c.jar"));
        jar(&a, &[("x/A.class", b"one"), ("x/B.class", b"two")], "Built-On: Monday\n");
        jar(&b, &[("x/B.class", b"two"), ("x/A.class", b"one")], "Built-On: Tuesday\n");
        jar(&c, &[("x/A.class", b"one"), ("x/B.class", b"changed")], "Built-On: Monday\n");
        assert!(same_contents(&a, &b));
        assert!(!same_contents(&a, &c));
        assert!(!same_contents(&a, &dir.join("missing.jar")));
        let injected = |id: &str| format!("architectury_inject_archers_common_{id}_76d4archerscommon3111211devjar/PlatformMethods.class");
        let (d, e) = (dir.join("d.jar"), dir.join("e.jar"));
        jar(&d, &[(&injected("5f62cd32852a4ae4872b31a2be6fd9c1"), b"same")], "");
        jar(&e, &[(&injected("bf52a38d2ea24c08a0c4fdd9e0bb84de"), b"names its own package")], "");
        assert!(same_contents(&d, &e), "Architectury's per-build ID isn't a difference");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
