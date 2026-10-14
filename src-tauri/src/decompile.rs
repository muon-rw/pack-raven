use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{bail, Context, Result};

type Key = (PathBuf, SystemTime, String);

// A class Vineflower can't finish in this long would only hang the tab.
const TIME_LIMIT: Duration = Duration::from_secs(60);

// A jar only changes when you update the mod, so we keep each decompiled class until then.
static CACHE: LazyLock<Mutex<HashMap<Key, String>>> = LazyLock::new(Default::default);

/// Decompiles a class inside an archive with Vineflower and returns the Java source.
pub fn decompile(java: &Path, vineflower: &Path, archive: &Path, inner: &str) -> Result<String> {
    let modified = std::fs::metadata(archive).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
    let key = (archive.to_path_buf(), modified, inner.to_string());
    if let Some(source) = CACHE.lock().unwrap().get(&key) {
        return Ok(source.clone());
    }
    let classes = crate::archive::class_family(archive, inner)?;
    let work = work_dir()?;
    let result = run(java, vineflower, &work, &classes);
    let _ = std::fs::remove_dir_all(&work);
    let source = result?;
    CACHE.lock().unwrap().insert(key, source.clone());
    Ok(source)
}

fn work_dir() -> Result<PathBuf> {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("packraven-decompile-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("in"))?;
    std::fs::create_dir_all(dir.join("out"))?;
    Ok(dir)
}

// Tauri resolves bundled resources to \\?\ paths on Windows, and some JVMs, like GraalVM 21,
// find the jar's manifest at such a path but can't load its classes.
fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with(r"UNC\") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

fn run(java: &Path, vineflower: &Path, work: &Path, classes: &[(String, Vec<u8>)]) -> Result<String> {
    let mut command = Command::new(java);
    // Vineflower starts a thread per core by default, which only costs memory for one class.
    // Like IntelliJ's decompiler, we skip malformed bytecode (common in obfuscated mods) rather
    // than fail, and check that anonymous classes really are local before inlining them.
    command.arg("-jar").arg(plain(vineflower)).args([
        "--log-level=warn",
        "--indent-string=    ",
        "--thread-count=1",
        "--ignore-invalid-bytecode=true",
        "--verify-anonymous-classes=true",
    ]);
    for (name, bytes) in classes {
        let path = work.join("in").join(name);
        std::fs::write(&path, bytes)?;
        command.arg(path);
    }
    command.arg(work.join("out"));
    // Vineflower's log goes to a file, so a chatty run can't fill a pipe while we wait on it.
    let log_path = work.join("log.txt");
    command.stdout(Stdio::null()).stderr(Stdio::from(std::fs::File::create(&log_path)?));
    crate::process::hide_window_std(&mut command);
    let mut child = command.spawn().with_context(|| format!("running {}", java.display()))?;
    let started = Instant::now();
    while child.try_wait()?.is_none() {
        if started.elapsed() > TIME_LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Vineflower took longer than {} seconds on this class, so we stopped it.", TIME_LIMIT.as_secs());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    // Vineflower names its output after the class, so we take whichever source it wrote.
    let written = std::fs::read_dir(work.join("out"))?.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e == "java"));
    let Some(written) = written else {
        let log = std::fs::read_to_string(&log_path).unwrap_or_default();
        if log.contains("UnsupportedClassVersionError") {
            bail!("Vineflower needs Java 17 or newer, and {} is older. Set a newer Java in Settings.", java.display());
        }
        let detail = log.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("it printed nothing");
        bail!("Vineflower couldn't decompile this class: {detail}");
    };
    Ok(std::fs::read_to_string(written)?)
}

/// Finds a Java to run Vineflower with: the one packraven runs the game with, or else `java`
/// on your PATH.
pub fn java(settings: &crate::settings::Settings) -> PathBuf {
    crate::launch::java(settings).unwrap_or_else(|_| PathBuf::from(if cfg!(windows) { "java.exe" } else { "java" }))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    // Needs Java on your PATH, so you run it on purpose: cargo test --lib -- --ignored decompiles
    #[test]
    #[ignore]
    fn decompiles_a_class_with_its_inner_classes() {
        // Canonical paths carry the \\?\ prefix on Windows, like Tauri's resource paths.
        let vineflower = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/vineflower.jar").canonicalize().unwrap();
        let java = std::env::var_os("PACKRAVEN_TEST_JAVA").map_or_else(|| "java".into(), std::path::PathBuf::from);
        let class = "org/jetbrains/java/decompiler/main/decompiler/ConsoleDecompiler.class";
        let source = super::decompile(&java, &vineflower, &vineflower, class).unwrap();
        assert!(source.contains("public class ConsoleDecompiler"), "{source}");
        assert!(super::decompile(&java, &vineflower, &vineflower, class).is_ok());
    }

    #[test]
    fn strips_the_verbatim_prefix_but_keeps_network_paths() {
        assert_eq!(super::plain(Path::new(r"\\?\C:\app\vineflower.jar")), Path::new(r"C:\app\vineflower.jar"));
        assert_eq!(super::plain(Path::new(r"\\?\UNC\server\share\a.jar")), Path::new(r"\\?\UNC\server\share\a.jar"));
    }
}
