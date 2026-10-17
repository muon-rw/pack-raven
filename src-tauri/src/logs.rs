use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result};
use chrono::TimeZone;
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

// We launch the client with vanilla's launcher log config, which prints each event to stdout as
// log4j XML for the launcher to parse. We can't drop the config, since for 1.12 to 1.16 it
// carries Mojang's Log4Shell fix, so we turn each event back into the lines latest.log would hold.
#[derive(Default)]
pub struct Log4jXmlDecoder {
    event: Option<String>,
}

impl Log4jXmlDecoder {
    pub fn feed(&mut self, line: &str) -> Vec<String> {
        match &mut self.event {
            Some(event) => {
                event.push('\n');
                event.push_str(line);
            }
            None if line.trim_start().starts_with("<log4j:Event ") => self.event = Some(line.to_string()),
            None => return vec![line.to_string()],
        }
        if !line.trim_end().ends_with("</log4j:Event>") {
            return Vec::new();
        }
        let event = self.event.take().unwrap_or_default();
        decode_log4j_event(&event).unwrap_or(event).lines().map(String::from).collect()
    }

    pub fn finish(&mut self) -> Vec<String> {
        self.event.take().map(|event| event.lines().map(String::from).collect()).unwrap_or_default()
    }
}

fn decode_log4j_event(xml: &str) -> Option<String> {
    let open = &xml[..xml.find('>')?];
    let attribute = |name: &str| {
        let start = open.find(&format!(" {name}=\""))? + name.len() + 3;
        Some(unescape_xml(&open[start..start + open[start..].find('"')?]))
    };
    let millis = attribute("timestamp")?.parse().ok()?;
    let time = chrono::Local.timestamp_millis_opt(millis).single()?.format("%H:%M:%S");
    let (thread, level, logger) = (attribute("thread")?, attribute("level")?, attribute("logger")?);
    let mut text = format!("[{time}] [{thread}/{level}] [{logger}]: {}", xml_element_text(xml, "log4j:Message").unwrap_or_default());
    if let Some(throwable) = xml_element_text(xml, "log4j:Throwable") {
        text.push('\n');
        text.push_str(throwable.trim_end());
    }
    Some(text)
}

fn xml_element_text(xml: &str, tag: &str) -> Option<String> {
    let start = xml.find(&format!("<{tag}>"))? + tag.len() + 2;
    let mut rest = &xml[start..start + xml[start..].find(&format!("</{tag}>"))?];
    let mut text = String::new();
    // log4j splits a CDATA section wherever the message itself contains "]]>".
    while let Some(open) = rest.find("<![CDATA[") {
        text.push_str(&unescape_xml(&rest[..open]));
        let body = &rest[open + 9..];
        let close = body.find("]]>").unwrap_or(body.len());
        text.push_str(&body[..close]);
        rest = &body[(close + 3).min(body.len())..];
    }
    text.push_str(&unescape_xml(rest));
    Some(text)
}

fn unescape_xml(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
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

    #[test]
    fn decodes_log4j_xml_events_into_log_lines() {
        let stdout = [
            "Plain line before log4j starts",
            r#"  <log4j:Event logger="Sodium-Workarounds" timestamp="1792257521618" level="WARN" thread="Render &quot;thread&quot;">"#,
            "    <log4j:Message><![CDATA[Applied workarounds: [NVIDIA]]]></log4j:Message>",
            "  </log4j:Event>",
            r#"  <log4j:Event logger="Minecraft" timestamp="1792257521618" level="ERROR" thread="main">"#,
            "    <log4j:Message><![CDATA[Two",
            "lines with ]]]]><![CDATA[> & <tags>]]></log4j:Message>",
            "    <log4j:Throwable><![CDATA[java.lang.Exception: boom",
            "\tat Foo.bar(Foo.java:1)",
            "]]></log4j:Throwable>",
            "  </log4j:Event>",
        ];
        let mut decoder = Log4jXmlDecoder::default();
        let lines: Vec<String> = stdout.iter().flat_map(|line| decoder.feed(line)).collect();
        let time = chrono::Local.timestamp_millis_opt(1792257521618).unwrap().format("%H:%M:%S");
        assert_eq!(
            lines,
            [
                "Plain line before log4j starts".to_string(),
                format!(r#"[{time}] [Render "thread"/WARN] [Sodium-Workarounds]: Applied workarounds: [NVIDIA]"#),
                format!("[{time}] [main/ERROR] [Minecraft]: Two"),
                "lines with ]]> & <tags>".to_string(),
                "java.lang.Exception: boom".to_string(),
                "\tat Foo.bar(Foo.java:1)".to_string(),
            ]
        );
    }

    #[test]
    fn flushes_an_unfinished_log4j_event_as_is() {
        let mut decoder = Log4jXmlDecoder::default();
        assert!(decoder.feed(r#"<log4j:Event logger="a" timestamp="1" level="INFO" thread="main">"#).is_empty());
        assert_eq!(decoder.finish(), [r#"<log4j:Event logger="a" timestamp="1" level="INFO" thread="main">"#]);
    }
}
