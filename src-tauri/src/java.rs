use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::http::CLIENT;

// Declared in the order we prefer them when several Javas of one version are installed.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(rename_all = "camelCase")]
pub enum JavaSource {
    CurseForge,
    Packraven,
    JavaHome,
    Path,
    System,
    IntelliJ,
    Gradle,
    Prism,
    Modrinth,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct JavaInstall {
    pub path: PathBuf,
    pub major: u32,
    pub version: String,
    pub distribution: String,
    pub source: JavaSource,
    pub foreign_arch: Option<String>,
}

// We set this at startup from Tauri's app data folder, which launch code can't reach without an AppHandle.
pub static DOWNLOAD_DIR: OnceLock<PathBuf> = OnceLock::new();

const EXECUTABLE: &str = if cfg!(windows) { "java.exe" } else { "java" };

fn executable(home: &Path) -> PathBuf {
    home.join("bin").join(EXECUTABLE)
}

// macOS JDKs keep the real home under Contents/Home, and CurseForge's older layout adds jre.bundle.
fn as_home(dir: &Path) -> Option<PathBuf> {
    [dir.to_path_buf(), dir.join("Contents").join("Home"), dir.join("jre.bundle").join("Contents").join("Home")]
        .into_iter()
        .find(|home| home.join("release").is_file() && executable(home).is_file())
}

fn homes_in(dir: &Path, depth: u32, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for path in entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        match as_home(&path) {
            Some(home) => found.push(home),
            None if depth > 1 => homes_in(&path, depth - 1, found),
            None => {}
        }
    }
}

fn scan(source: JavaSource, dir: &Path, depth: u32) -> Vec<(JavaSource, PathBuf)> {
    let mut homes = Vec::new();
    homes_in(dir, depth, &mut homes);
    homes.into_iter().map(|home| (source, home)).collect()
}

// We only trust a java on PATH that resolves into a JDK with a release file. That skips stubs like
// macOS's /usr/bin/java, which opens an install prompt when run without a JDK.
fn path_homes() -> Vec<PathBuf> {
    let Some(path) = std::env::var_os("PATH") else { return Vec::new() };
    std::env::split_paths(&path)
        .map(|dir| dir.join(EXECUTABLE))
        .filter(|java| java.is_file())
        .filter_map(|java| {
            let resolved = if cfg!(windows) { java } else { std::fs::canonicalize(&java).ok()? };
            as_home(resolved.parent()?.parent()?)
        })
        .collect()
}

fn system_homes() -> Vec<(JavaSource, PathBuf)> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut found = Vec::new();
    if cfg!(windows) {
        const VENDORS: [&str; 14] =
            ["Java", "Eclipse Adoptium", "Eclipse Foundation", "AdoptOpenJDK", "Zulu", "Microsoft", "Amazon Corretto", "BellSoft", "Semeru", "IBM", "GraalVM", "OpenJDK", "SapMachine", "RedHat"];
        let roots = ["ProgramFiles", "ProgramFiles(x86)"].into_iter().filter_map(std::env::var_os).map(PathBuf::from);
        let roots = roots.chain(dirs::data_local_dir().map(|d| d.join("Programs")));
        for root in roots {
            for vendor in VENDORS {
                found.extend(scan(JavaSource::System, &root.join(vendor), 2));
            }
        }
        found.extend(scan(JavaSource::System, &home.join("scoop").join("apps"), 2));
    } else if cfg!(target_os = "macos") {
        found.extend(scan(JavaSource::System, Path::new("/Library/Java/JavaVirtualMachines"), 1));
        found.extend(scan(JavaSource::System, &home.join("Library/Java/JavaVirtualMachines"), 1));
        // Homebrew's openjdk formulae are keg-only, so their JDKs aren't on PATH.
        for opt in ["/opt/homebrew/opt", "/usr/local/opt"] {
            let Ok(entries) = std::fs::read_dir(opt) else { continue };
            for entry in entries.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("openjdk")) {
                found.extend(as_home(&entry.path().join("libexec").join("openjdk.jdk")).map(|h| (JavaSource::System, h)));
            }
        }
    } else {
        found.extend(scan(JavaSource::System, Path::new("/usr/lib/jvm"), 1));
    }
    found.extend(scan(JavaSource::System, &home.join(".sdkman").join("candidates").join("java"), 1));
    found
}

fn candidate_homes(install_dir: &Path) -> Vec<(JavaSource, PathBuf)> {
    let home = dirs::home_dir().unwrap_or_default();
    let data = dirs::data_dir().unwrap_or_default();
    let mut found = Vec::new();
    found.extend(scan(JavaSource::CurseForge, &install_dir.join("java"), 1));
    found.extend(scan(JavaSource::CurseForge, &install_dir.join("runtime"), 3));
    if let Some(dir) = DOWNLOAD_DIR.get() {
        found.extend(scan(JavaSource::Packraven, dir, 1));
    }
    if let Some(java_home) = std::env::var_os("JAVA_HOME") {
        found.extend(as_home(Path::new(&java_home)).map(|h| (JavaSource::JavaHome, h)));
    }
    found.extend(path_homes().into_iter().map(|h| (JavaSource::Path, h)));
    found.extend(system_homes());
    found.extend(scan(JavaSource::IntelliJ, &home.join(".jdks"), 1));
    found.extend(scan(JavaSource::Gradle, &home.join(".gradle").join("jdks"), 2));
    found.extend(scan(JavaSource::Prism, &data.join("PrismLauncher").join("java"), 2));
    found.extend(scan(JavaSource::Modrinth, &data.join("ModrinthApp").join("meta").join("java_versions"), 2));
    found
}

fn read_release(home: &Path) -> Option<BTreeMap<String, String>> {
    let text = std::fs::read_to_string(home.join("release")).ok()?;
    Some(
        text.lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.trim().to_string(), value.trim().trim_matches('"').to_string()))
            .collect(),
    )
}

// Java 8 and older report versions like 1.8.0_402.
fn major_of(version: &str) -> Option<u32> {
    let mut parts = version.split(['.', '_', '+', '-']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 { parts.next()?.parse().ok() } else { Some(first) }
}

fn distribution(release: &BTreeMap<String, String>) -> String {
    let field = |key: &str| release.get(key).map(String::as_str).unwrap_or_default();
    if release.contains_key("GRAALVM_VERSION") || field("JAVA_RUNTIME_VERSION").contains("jvmci") {
        return "GraalVM".into();
    }
    let vendor = format!("{} {}", field("IMPLEMENTOR"), field("IMPLEMENTOR_VERSION"));
    const NAMES: [(&str, &str); 14] = [
        ("GraalVM", "GraalVM"),
        ("Temurin", "Temurin"),
        ("AdoptOpenJDK", "AdoptOpenJDK"),
        ("Corretto", "Corretto"),
        ("Zulu", "Zulu"),
        ("Azul", "Zulu"),
        ("Microsoft", "Microsoft"),
        ("BellSoft", "Liberica"),
        ("JetBrains", "JetBrains Runtime"),
        ("Semeru", "Semeru"),
        ("IBM", "Semeru"),
        ("SAP", "SapMachine"),
        ("Red Hat", "Red Hat"),
        ("Oracle", "Oracle"),
    ];
    NAMES.iter().find(|(needle, _)| vendor.contains(needle)).map_or("OpenJDK", |(_, name)| name).into()
}

fn normalized_arch(arch: &str) -> &str {
    match arch {
        "amd64" | "x64" => "x86_64",
        "arm64" => "aarch64",
        other => other,
    }
}

fn inspect(source: JavaSource, home: &Path) -> Option<JavaInstall> {
    let release = read_release(home)?;
    let version = release.get("JAVA_VERSION")?.clone();
    let foreign_arch = release
        .get("OS_ARCH")
        .map(|arch| normalized_arch(arch).to_string())
        .filter(|arch| arch != normalized_arch(std::env::consts::ARCH));
    Some(JavaInstall { path: executable(home), major: major_of(&version)?, distribution: distribution(&release), version, source, foreign_arch })
}

fn version_key(version: &str) -> Vec<u32> {
    version.split(['.', '_', '+', '-']).map_while(|part| part.parse().ok()).collect()
}

fn dedupe_key(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = resolved.to_string_lossy().into_owned();
    if cfg!(windows) { text.to_lowercase() } else { text }
}

/// Lists every Java on this computer that we can identify, newest version first.
pub fn detect(install_dir: &Path) -> Vec<JavaInstall> {
    let mut seen = HashSet::new();
    let mut installs: Vec<JavaInstall> = candidate_homes(install_dir)
        .into_iter()
        .filter(|(_, home)| seen.insert(dedupe_key(home)))
        .filter_map(|(source, home)| inspect(source, &home))
        .collect();
    installs.sort_by(|a, b| b.major.cmp(&a.major).then(a.source.cmp(&b.source)).then_with(|| version_key(&b.version).cmp(&version_key(&a.version))));
    installs
}

/// Picks the Java we use for a Java version you haven't chosen one for: CurseForge's first, and
/// builds for another CPU last, since macOS runs those slowly under Rosetta.
pub fn automatic(installs: &[JavaInstall], major: u32) -> Option<&JavaInstall> {
    installs.iter().filter(|java| java.major == major).min_by_key(|java| (java.foreign_arch.is_some(), java.source))
}

// GraalVM 21 runs everything that needs Java 17, and faster than Java 17 does. Oracle's free
// GraalVM 17 builds also stopped getting updates.
pub fn graalvm_for(major: u32) -> Option<u32> {
    match major {
        17 | 21 => Some(21),
        25 => Some(25),
        _ => None,
    }
}

fn graalvm_platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("windows-x64_bin.zip"),
        ("macos", "aarch64") => Some("macos-aarch64_bin.tar.gz"),
        ("macos", "x86_64") => Some("macos-x64_bin.tar.gz"),
        ("linux", "x86_64") => Some("linux-x64_bin.tar.gz"),
        ("linux", "aarch64") => Some("linux-aarch64_bin.tar.gz"),
        _ => None,
    }
}

/// Maps each Java version Minecraft needs to the GraalVM version we can download for it.
pub fn downloads() -> BTreeMap<u32, u32> {
    if graalvm_platform().is_none() {
        return BTreeMap::new();
    }
    [8, 16, 17, 21, 25].into_iter().filter_map(|major| Some((major, graalvm_for(major)?))).collect()
}

static DOWNLOADS: LazyLock<Mutex<BTreeMap<u32, Arc<AtomicBool>>>> = LazyLock::new(Mutex::default);

struct DownloadSlot(u32);

impl Drop for DownloadSlot {
    fn drop(&mut self) {
        DOWNLOADS.lock().unwrap().remove(&self.0);
    }
}

pub fn cancel_download(graalvm: u32) {
    if let Some(cancelled) = DOWNLOADS.lock().unwrap().get(&graalvm) {
        cancelled.store(true, Ordering::Relaxed);
    }
}

/// Downloads Oracle GraalVM for `major` and returns it, reporting bytes received and the total
/// size as it goes.
pub async fn download_graalvm(major: u32, progress: impl Fn(u64, Option<u64>)) -> Result<JavaInstall> {
    let root = DOWNLOAD_DIR.get().context("packraven has no folder to download Java into")?;
    let graalvm = graalvm_for(major).with_context(|| format!("Oracle doesn't publish GraalVM for Java {major}"))?;
    let platform = graalvm_platform().context("Oracle doesn't publish GraalVM for this computer's CPU")?;
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut downloads = DOWNLOADS.lock().unwrap();
        if downloads.contains_key(&graalvm) {
            bail!("packraven is already downloading GraalVM {graalvm}.");
        }
        downloads.insert(graalvm, cancelled.clone());
    }
    let _slot = DownloadSlot(graalvm);

    let url = format!("https://download.oracle.com/graalvm/{graalvm}/latest/graalvm-jdk-{graalvm}_{platform}");
    let checksum = CLIENT.get(format!("{url}.sha256")).send().await?.error_for_status()?.text().await.context("reading GraalVM's checksum")?;
    let expected = checksum.split_whitespace().next().context("Oracle didn't list a checksum for GraalVM")?.to_lowercase();

    tokio::fs::create_dir_all(root).await?;
    let archive = root.join(format!(".graalvm-{graalvm}-download"));
    let response = CLIENT.get(&url).send().await?.error_for_status().context("downloading GraalVM")?;
    let total = response.content_length();
    let mut file = tokio::fs::File::create(&archive).await?;
    let mut hasher = Sha256::new();
    let mut body = response.bytes_stream();
    let (mut received, mut reported) = (0u64, 0u64);
    while let Some(chunk) = body.next().await {
        if cancelled.load(Ordering::Relaxed) {
            drop(file);
            let _ = tokio::fs::remove_file(&archive).await;
            bail!("Download cancelled.");
        }
        let chunk = chunk.context("downloading GraalVM")?;
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        received += chunk.len() as u64;
        // A report per chunk would flood the UI with tens of thousands of events.
        if received - reported >= 1 << 20 {
            reported = received;
            progress(received, total);
        }
    }
    file.flush().await?;
    drop(file);
    if hex::encode(hasher.finalize()) != expected {
        let _ = tokio::fs::remove_file(&archive).await;
        bail!("The GraalVM download doesn't match Oracle's checksum, so packraven didn't install it. Try again.");
    }

    let staging = root.join(format!(".graalvm-{graalvm}-unpacking"));
    let home = tauri::async_runtime::spawn_blocking({
        let (archive, staging, root) = (archive.clone(), staging.clone(), root.to_path_buf());
        move || unpack(&archive, &staging, &root)
    })
    .await?;
    let _ = std::fs::remove_file(&archive);
    let _ = std::fs::remove_dir_all(&staging);
    inspect(JavaSource::Packraven, &home?).context("the GraalVM download has no Java in it")
}

// Oracle's archives hold one folder named after the exact version, like graalvm-jdk-21.0.9+7.1, so
// we keep that name and leave versions you downloaded before alone.
fn unpack(archive: &Path, staging: &Path, root: &Path) -> Result<PathBuf> {
    let _ = std::fs::remove_dir_all(staging);
    std::fs::create_dir_all(staging)?;
    let file = std::fs::File::open(archive)?;
    if cfg!(windows) {
        zip::ZipArchive::new(file)?.extract(staging).context("unpacking GraalVM")?;
    } else {
        tar::Archive::new(flate2::read::GzDecoder::new(file)).unpack(staging).context("unpacking GraalVM")?;
    }
    let folder = std::fs::read_dir(staging)?.flatten().map(|e| e.path()).find(|p| p.is_dir()).context("the GraalVM download is empty")?;
    let target = root.join(folder.file_name().context("the GraalVM download is empty")?);
    if as_home(&target).is_none() {
        let _ = std::fs::remove_dir_all(&target);
        rename_when_unlocked(&folder, &target)?;
    }
    as_home(&target).context("the GraalVM download has no Java in it")
}

// Windows Defender scans new files as we write them, and Windows refuses to move a folder while a
// file in it is open.
fn rename_when_unlocked(from: &Path, to: &Path) -> Result<()> {
    let mut attempt = 0;
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(_) if attempt < 20 => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            Err(error) => return Err(error).with_context(|| format!("moving GraalVM to {}", to.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_major_versions_old_and_new() {
        assert_eq!(major_of("1.8.0_402"), Some(8));
        assert_eq!(major_of("17.0.16"), Some(17));
        assert_eq!(major_of("21"), Some(21));
        assert_eq!(major_of("25-ea"), Some(25));
    }

    #[test]
    fn names_distributions_from_the_release_file() {
        let release = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>();
        assert_eq!(distribution(&release(&[("IMPLEMENTOR", "Eclipse Adoptium"), ("IMPLEMENTOR_VERSION", "Temurin-21.0.8+9")])), "Temurin");
        assert_eq!(distribution(&release(&[("IMPLEMENTOR", "Amazon.com Inc."), ("IMPLEMENTOR_VERSION", "Corretto-18.0.2.9.1")])), "Corretto");
        assert_eq!(distribution(&release(&[("IMPLEMENTOR", "Oracle Corporation"), ("GRAALVM_VERSION", "23.1.7")])), "GraalVM");
        assert_eq!(distribution(&release(&[("IMPLEMENTOR", "Oracle Corporation"), ("JAVA_RUNTIME_VERSION", "25.0.2+10-LTS-jvmci-b01")])), "GraalVM");
        assert_eq!(distribution(&release(&[("IMPLEMENTOR", "Azul Systems, Inc."), ("IMPLEMENTOR_VERSION", "Zulu21.30+15-CA")])), "Zulu");
        assert_eq!(distribution(&release(&[])), "OpenJDK");
    }

    #[test]
    fn prefers_curseforge_then_native_builds() {
        let java = |source, foreign: bool| JavaInstall {
            path: PathBuf::from(format!("{source:?}{foreign}")),
            major: 21,
            version: "21.0.1".into(),
            distribution: "Temurin".into(),
            source,
            foreign_arch: foreign.then(|| "x86_64".into()),
        };
        let installs = [java(JavaSource::CurseForge, true), java(JavaSource::Path, false), java(JavaSource::System, false)];
        assert_eq!(automatic(&installs, 21).unwrap().source, JavaSource::Path);
        assert!(automatic(&installs, 17).is_none());
    }
}
