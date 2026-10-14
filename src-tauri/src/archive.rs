use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::files::Entry;

const EXTENSIONS: [&str; 2] = ["jar", "zip"];

trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}
type Archive = ZipArchive<Box<dyn ReadSeek>>;

pub fn is_archive(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

/// Splits a pack-relative path that runs through a jar or zip into the archive's file on disk
/// and the path inside it, which is empty for the archive itself. Returns None for paths that
/// never enter an archive.
pub fn split(root: &Path, relative: &str) -> Option<(PathBuf, String)> {
    let parts: Vec<&str> = relative.split('/').filter(|p| !p.is_empty()).collect();
    let mut path = root.to_path_buf();
    for (index, part) in parts.iter().enumerate() {
        path.push(part);
        if is_archive(part) && path.is_file() {
            return Some((path, parts[index + 1..].join("/")));
        }
    }
    None
}

fn open_file(file: &Path) -> Result<Archive> {
    let reader: Box<dyn ReadSeek> = Box::new(BufReader::new(File::open(file).with_context(|| format!("opening {}", file.display()))?));
    ZipArchive::new(reader).with_context(|| format!("reading {} as a zip", file.display()))
}

// Mods often ship libraries as jars inside their jar, so we read a nested archive into memory
// and keep going. With descend_last set, a path that ends at a nested archive opens it too.
fn open(file: &Path, inner: &str, descend_last: bool) -> Result<(Archive, String)> {
    let mut archive = open_file(file)?;
    let mut rest = inner.to_string();
    loop {
        let parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
        let last = if descend_last { parts.len() } else { parts.len().saturating_sub(1) };
        let nested = (1..=last).find(|&end| is_archive(parts[end - 1]) && archive.index_for_name(&parts[..end].join("/")).is_some());
        let Some(end) = nested else { return Ok((archive, parts.join("/"))) };
        let mut bytes = Vec::new();
        archive.by_name(&parts[..end].join("/"))?.read_to_end(&mut bytes)?;
        let reader: Box<dyn ReadSeek> = Box::new(Cursor::new(bytes));
        archive = ZipArchive::new(reader).with_context(|| format!("reading {} as a zip", parts[end - 1]))?;
        rest = parts[end..].join("/");
    }
}

// Some zips use backslashes or leading slashes in entry names, and a hostile one can use "..".
fn clean_name(name: &str) -> Option<String> {
    let name = name.replace('\\', "/");
    let parts: Vec<&str> = name.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    (!parts.is_empty() && !parts.contains(&"..")).then(|| parts.join("/"))
}

/// Lists one folder level inside an archive. `relative` is the folder's pack-relative path,
/// which prefixes the entries' paths.
pub fn list(file: &Path, inner: &str, relative: &str) -> Result<Vec<Entry>> {
    let (archive, dir) = open(file, inner, true)?;
    let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
    // A zip may list a file without listing its folders, so we infer folders from the paths.
    let mut children: BTreeMap<String, (bool, u64)> = BTreeMap::new();
    for index in 0..archive.len() {
        let Ok(entry) = archive.by_index_data(index) else { continue };
        let Some(name) = entry.name().ok().and_then(|n| clean_name(&n)) else { continue };
        let Some(below) = name.strip_prefix(&prefix) else { continue };
        match below.split_once('/') {
            Some((folder, _)) => {
                children.insert(folder.to_string(), (true, 0));
            }
            None if entry.is_dir() => {
                children.insert(below.to_string(), (true, 0));
            }
            None => {
                children.entry(below.to_string()).or_insert((false, entry.size()));
            }
        }
    }
    let base = relative.trim_end_matches('/');
    let mut entries: Vec<Entry> = children
        .into_iter()
        .map(|(name, (is_dir, size))| Entry {
            path: format!("{base}/{name}"),
            archive: !is_dir && is_archive(&name),
            archived: true,
            name,
            is_dir,
            size,
            ignored: false,
        })
        .collect();
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(entries)
}

/// Reads one file inside an archive, refusing files larger than `limit`.
pub fn read(file: &Path, inner: &str, limit: u64) -> Result<Vec<u8>> {
    let (mut archive, name) = open(file, inner, false)?;
    let mut entry = archive.by_name(&name).with_context(|| format!("{name} isn't in {}", file.display()))?;
    if entry.size() > limit {
        bail!("{name} is too large to open here");
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub fn size(file: &Path, inner: &str) -> Result<u64> {
    let (mut archive, name) = open(file, inner, false)?;
    Ok(archive.by_name(&name).with_context(|| format!("{name} isn't in {}", file.display()))?.size())
}

/// Reads a class file with the inner classes the compiler split out of it, like `Outer$Inner`,
/// so a decompiler can put them back together.
pub fn class_family(file: &Path, inner: &str) -> Result<Vec<(String, Vec<u8>)>> {
    let (mut archive, name) = open(file, inner, false)?;
    let (dir, file_name) = name.rsplit_once('/').map_or(("", name.as_str()), |(d, f)| (d, f));
    let stem = file_name.strip_suffix(".class").context("only class files can be decompiled")?;
    let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
    let names: Vec<String> = archive
        .file_names()
        .filter_map(|n| n.ok().map(|n| n.into_owned()))
        .filter(|n| {
            n.strip_prefix(&prefix).is_some_and(|rest| !rest.contains('/') && (rest == file_name || (rest.starts_with(&format!("{stem}$")) && rest.ends_with(".class"))))
        })
        .collect();
    names
        .into_iter()
        .map(|n| {
            let mut bytes = Vec::new();
            archive.by_name(&n)?.read_to_end(&mut bytes)?;
            Ok((n.rsplit('/').next().unwrap_or(&n).to_string(), bytes))
        })
        .collect()
}

/// Returns a path in `dir` named `stem` plus `extension`, numbered like "name (2).zip" when
/// that name is taken.
pub fn unused_path(dir: &Path, stem: &str, extension: &str) -> PathBuf {
    (1..)
        .map(|n| if n == 1 { format!("{stem}{extension}") } else { format!("{stem} ({n}){extension}") })
        .map(|name| dir.join(name))
        .find(|p| !p.exists())
        .expect("some numbered name is free")
}

pub fn is_gzip(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("gz"))
}

/// Decompresses gzip data, or returns None when it inflates past `limit` bytes.
pub fn gunzip(bytes: &[u8], limit: u64) -> Result<Option<Vec<u8>>> {
    let mut data = Vec::new();
    // Minecraft writes one gzip member per log, but other tools append several.
    flate2::read::MultiGzDecoder::new(bytes).take(limit + 1).read_to_end(&mut data).context("decompressing")?;
    Ok((data.len() as u64 <= limit).then_some(data))
}

// "2026-10-13-1.log.gz" decompresses to "2026-10-13-1.log", numbered if that name is taken.
fn gunzip_file(file: &Path) -> Result<PathBuf> {
    let parent = file.parent().context("the file has no parent folder")?;
    let inner = Path::new(file.file_stem().context("the file has no name")?);
    let stem = inner.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let extension = inner.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let target = unused_path(parent, &stem, &extension);
    let result = (|| {
        let mut decoder = flate2::read::MultiGzDecoder::new(BufReader::new(File::open(file)?));
        std::io::copy(&mut decoder, &mut File::create(&target)?).context("decompressing")?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&target);
        return Err(e);
    }
    Ok(target)
}

/// Extracts a zip or jar into a new folder next to it, named after it, or decompresses a gzip
/// file next to itself. Returns what it created.
pub fn extract(file: &Path) -> Result<PathBuf> {
    if is_gzip(&file.to_string_lossy()) {
        return gunzip_file(file);
    }
    let parent = file.parent().context("the archive has no parent folder")?;
    let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "extracted".into());
    let target = unused_path(parent, &stem, "");
    let result = open_file(file).and_then(|mut archive| archive.extract(&target).context("extracting"));
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&target);
        return Err(e);
    }
    Ok(target)
}

/// Compresses the paths into a new zip in `dir`, named after the first one, and returns it.
pub fn compress(sources: &[PathBuf], dir: &Path) -> Result<PathBuf> {
    let first = sources.first().context("pick something to compress")?;
    let stem = if first.is_dir() { first.file_name() } else { first.file_stem() }.map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Archive".into());
    let target = unused_path(dir, &stem, ".zip");
    let result = write_zip(sources, dir, &target);
    if let Err(e) = result {
        let _ = std::fs::remove_file(&target);
        return Err(e);
    }
    Ok(target)
}

fn write_zip(sources: &[PathBuf], dir: &Path, target: &Path) -> Result<()> {
    let mut zip = ZipWriter::new(File::create(target).with_context(|| format!("creating {}", target.display()))?);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated).large_file(true);
    for source in sources {
        for entry in walkdir::WalkDir::new(source) {
            let entry = entry?;
            let name = entry.path().strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
            if entry.file_type().is_dir() {
                zip.add_directory(name, options)?;
            } else {
                zip.start_file(name, options)?;
                std::io::copy(&mut File::open(entry.path())?, &mut zip)?;
            }
        }
    }
    zip.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = ZipWriter::new(File::create(path).unwrap());
        for (name, bytes) in entries {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut zip, bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    fn nested_jar() -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(&mut bytes);
        zip.start_file("lib/Inner.txt", SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut zip, b"nested").unwrap();
        zip.finish().unwrap();
        bytes.into_inner()
    }

    #[test]
    fn browses_and_reads_archives_including_nested_jars() {
        let root = std::env::temp_dir().join("packraven-archive-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("mods")).unwrap();
        let nested = nested_jar();
        write(
            &root.join("mods/a.jar"),
            &[("assets/a/lang/en_us.json", b"{}"), ("a/Thing.class", b"outer"), ("a/Thing$Part.class", b"inner"), ("a/Other.class", b"x"), ("META-INF/jarjar/lib.jar", &nested)],
        );

        assert_eq!(split(&root, "mods/a.jar/assets/a"), Some((root.join("mods/a.jar"), "assets/a".to_string())));
        assert_eq!(split(&root, "mods/a.jar").map(|(_, inner)| inner), Some(String::new()));
        assert!(split(&root, "mods").is_none());

        let top = list(&root.join("mods/a.jar"), "", "mods/a.jar").unwrap();
        let names: Vec<_> = top.iter().map(|e| (e.path.as_str(), e.is_dir)).collect();
        assert_eq!(names, [("mods/a.jar/a", true), ("mods/a.jar/assets", true), ("mods/a.jar/META-INF", true)]);
        let jarjar = list(&root.join("mods/a.jar"), "META-INF/jarjar", "mods/a.jar/META-INF/jarjar").unwrap();
        assert!(jarjar[0].archive && jarjar[0].archived);
        let inside = list(&root.join("mods/a.jar"), "META-INF/jarjar/lib.jar/lib", "x").unwrap();
        assert_eq!(inside[0].name, "Inner.txt");

        assert_eq!(read(&root.join("mods/a.jar"), "assets/a/lang/en_us.json", 100).unwrap(), b"{}");
        assert_eq!(read(&root.join("mods/a.jar"), "META-INF/jarjar/lib.jar/lib/Inner.txt", 100).unwrap(), b"nested");
        assert!(read(&root.join("mods/a.jar"), "assets/a/lang/en_us.json", 1).is_err());

        let family: Vec<String> = class_family(&root.join("mods/a.jar"), "a/Thing.class").unwrap().into_iter().map(|(n, _)| n).collect();
        assert_eq!(family.len(), 2);
        assert!(family.contains(&"Thing$Part.class".to_string()) && !family.contains(&"Other.class".to_string()));
    }

    #[test]
    fn compresses_and_extracts_without_clobbering() {
        let root = std::env::temp_dir().join("packraven-zip-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("pack/data/x")).unwrap();
        std::fs::write(root.join("pack/data/x/a.json"), "{}").unwrap();
        std::fs::write(root.join("pack/pack.mcmeta"), "{}").unwrap();

        let zip = compress(&[root.join("pack")], &root).unwrap();
        assert_eq!(zip, root.join("pack.zip"));
        assert_eq!(compress(&[root.join("pack")], &root).unwrap(), root.join("pack (2).zip"));

        let extracted = extract(&zip).unwrap();
        assert_eq!(extracted, root.join("pack (2)"));
        assert_eq!(std::fs::read_to_string(extracted.join("pack/data/x/a.json")).unwrap(), "{}");
    }

    #[test]
    fn decompresses_gzipped_logs_without_clobbering() {
        let root = std::env::temp_dir().join("packraven-gzip-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, b"[12:00:00] [main/INFO]: Loading\n").unwrap();
        let gz = encoder.finish().unwrap();
        std::fs::write(root.join("2026-10-13-1.log.gz"), &gz).unwrap();

        assert_eq!(gunzip(&gz, 1000).unwrap().as_deref(), Some(&b"[12:00:00] [main/INFO]: Loading\n"[..]));
        assert_eq!(gunzip(&gz, 5).unwrap(), None);

        assert_eq!(extract(&root.join("2026-10-13-1.log.gz")).unwrap(), root.join("2026-10-13-1.log"));
        assert_eq!(extract(&root.join("2026-10-13-1.log.gz")).unwrap(), root.join("2026-10-13-1 (2).log"));
        assert_eq!(std::fs::read_to_string(root.join("2026-10-13-1.log")).unwrap(), "[12:00:00] [main/INFO]: Loading\n");
    }

    #[test]
    fn rejects_entry_names_that_escape() {
        assert_eq!(clean_name("/a\\b/./c"), Some("a/b/c".to_string()));
        assert_eq!(clean_name("../evil"), None);
    }
}
