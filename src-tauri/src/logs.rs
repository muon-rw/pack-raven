use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

// A modded debug.log can run to hundreds of megabytes, and only its end matters while a game
// runs, so past this size we show the last part.
const MAX_LOG_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogChunk {
    /// Whole lines, ending with a line break.
    text: String,
    /// The byte offset to read from next time, just past the last whole line.
    offset: u64,
    /// Whether `text` replaces everything shown so far rather than adding to it.
    reset: bool,
    /// Whether we left out the start of the log because it's too large.
    truncated: bool,
    /// Whether the file can grow, so it's worth reading again.
    followable: bool,
}

/// Reads a log from `offset` on, or from the start when `offset` is 0 or the file has shrunk,
/// as it does when Minecraft starts a new latest.log.
pub fn read(root: &Path, relative: &str, offset: u64) -> Result<LogChunk> {
    let inside = crate::archive::split(root, relative).is_some_and(|(_, inner)| !inner.is_empty());
    if inside || crate::archive::is_gzip(relative) {
        return read_whole(root, relative);
    }
    let path = crate::files::resolve_entry(root, relative)?;
    let mut file = File::open(&path).with_context(|| format!("opening {relative}"))?;
    let length = file.metadata()?.len();
    let reset = offset == 0 || offset > length;
    let start = if reset { length.saturating_sub(MAX_LOG_BYTES) } else { offset };
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity((length - start) as usize);
    file.take(length - start).read_to_end(&mut bytes)?;
    // Starting partway through, we drop the partial first line.
    let skipped = if reset && start > 0 { bytes.iter().position(|b| *b == b'\n').map_or(bytes.len(), |i| i + 1) } else { 0 };
    // Minecraft may be partway through writing the last line, so we stop at the last line
    // break and read the rest next time.
    let end = bytes.iter().rposition(|b| *b == b'\n').map_or(skipped, |i| (i + 1).max(skipped));
    Ok(LogChunk {
        text: String::from_utf8_lossy(&bytes[skipped..end]).into_owned(),
        offset: start + end as u64,
        reset,
        truncated: reset && start > 0,
        followable: true,
    })
}

fn read_whole(root: &Path, relative: &str) -> Result<LogChunk> {
    let bytes = crate::files::bytes(root, relative, MAX_LOG_BYTES)?;
    let (data, truncated) = if crate::archive::is_gzip(relative) {
        let mut data = Vec::new();
        flate2::read::MultiGzDecoder::new(&bytes[..]).take(MAX_LOG_BYTES + 1).read_to_end(&mut data).context("decompressing")?;
        let truncated = data.len() as u64 > MAX_LOG_BYTES;
        data.truncate(MAX_LOG_BYTES as usize);
        (data, truncated)
    } else {
        (bytes, false)
    };
    Ok(LogChunk { text: String::from_utf8_lossy(&data).into_owned(), offset: 0, reset: true, truncated, followable: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn follows_a_growing_log_and_starts_over_when_it_is_replaced() {
        let root = std::env::temp_dir().join("packraven-log-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("logs")).unwrap();
        let path = root.join("logs/latest.log");
        std::fs::write(&path, "[00:00:01] [main/INFO]: one\n[00:00:02] [main/WARN]: tw").unwrap();

        let first = read(&root, "logs/latest.log", 0).unwrap();
        assert_eq!(first.text, "[00:00:01] [main/INFO]: one\n");
        assert!(first.reset && first.followable);

        std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"o\n").unwrap();
        let next = read(&root, "logs/latest.log", first.offset).unwrap();
        assert_eq!(next.text, "[00:00:02] [main/WARN]: two\n");
        assert!(!next.reset);

        std::fs::write(&path, "new\n").unwrap();
        let replaced = read(&root, "logs/latest.log", next.offset).unwrap();
        assert_eq!(replaced.text, "new\n");
        assert!(replaced.reset);
    }

    #[test]
    fn reads_gzipped_logs_whole() {
        let root = std::env::temp_dir().join("packraven-log-gz-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"a\nb\n").unwrap();
        std::fs::write(root.join("old.log.gz"), encoder.finish().unwrap()).unwrap();

        let chunk = read(&root, "old.log.gz", 0).unwrap();
        assert_eq!(chunk.text, "a\nb\n");
        assert!(!chunk.followable && !chunk.truncated);
    }
}
