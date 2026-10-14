use std::io::Read;

use anyhow::{bail, Context, Result};
use serde::Serialize;

// Arrays like heightmaps and block states can hold tens of thousands of numbers, which nobody
// reads one by one, so we send the first few.
const MAX_ARRAY_ITEMS: usize = 256;
const MAX_TAGS: usize = 2_000_000;
const MAX_DEPTH: usize = 512;
const MAX_INFLATED: u64 = 256 * 1024 * 1024;

#[derive(Serialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    // JavaScript numbers can't hold every long, so we send them as text.
    Long(String),
    Float(f32),
    Double(f64),
    ByteArray(Array<i8>),
    String(String),
    List(List),
    Compound(Vec<(String, Tag)>),
    IntArray(Array<i32>),
    LongArray(Array<String>),
}

#[derive(Serialize)]
pub struct Array<T> {
    length: usize,
    items: Vec<T>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct List {
    item_type: &'static str,
    items: Vec<Tag>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    compression: &'static str,
    root_name: String,
    root: Tag,
}

/// Parses a Java Edition NBT file, which Minecraft stores gzipped, zlib-compressed or plain.
pub fn parse(bytes: &[u8]) -> Result<Document> {
    let (compression, data) = match bytes {
        [0x1f, 0x8b, ..] => ("gzip", inflate(flate2::read::GzDecoder::new(bytes))?),
        [0x78, 0x01 | 0x5e | 0x9c | 0xda, ..] => ("zlib", inflate(flate2::read::ZlibDecoder::new(bytes))?),
        _ => ("none", bytes.to_vec()),
    };
    let mut reader = Reader { data: &data, at: 0, tags: 0 };
    let kind = reader.u8()?;
    if kind != 10 {
        bail!("This file doesn't start with an NBT compound, so it isn't Java Edition NBT.");
    }
    let root_name = reader.string()?;
    let root = reader.tag(kind, 0)?;
    Ok(Document { compression, root_name, root })
}

fn inflate(mut decoder: impl Read) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    (&mut decoder).take(MAX_INFLATED + 1).read_to_end(&mut data).context("decompressing")?;
    if data.len() as u64 > MAX_INFLATED {
        bail!("This file is too large to show once decompressed.");
    }
    Ok(data)
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
    tags: usize,
}

impl Reader<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8]> {
        let end = self.at.checked_add(count).filter(|&end| end <= self.data.len());
        let Some(end) = end else { bail!("This file ends in the middle of a tag, so it's damaged or isn't NBT.") };
        let slice = &self.data[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().expect("take returns N bytes"))
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.array()?))
    }

    fn length(&mut self) -> Result<usize> {
        let length = self.i32()?;
        if length < 0 {
            bail!("This file has a negative length, so it's damaged or isn't NBT.");
        }
        Ok(length as usize)
    }

    // NBT uses Java's modified UTF-8, which only differs from UTF-8 for nulls and emoji.
    fn string(&mut self) -> Result<String> {
        let length = u16::from_be_bytes(self.array()?) as usize;
        Ok(String::from_utf8_lossy(self.take(length)?).into_owned())
    }

    fn numbers<T, const N: usize>(&mut self, convert: impl Fn([u8; N]) -> T) -> Result<Array<T>> {
        let length = self.length()?;
        let bytes = self.take(length.checked_mul(N).context("array too large")?)?;
        let items = bytes.as_chunks::<N>().0.iter().take(MAX_ARRAY_ITEMS).map(|c| convert(*c)).collect();
        Ok(Array { length, items })
    }

    fn tag(&mut self, kind: u8, depth: usize) -> Result<Tag> {
        self.tags += 1;
        if self.tags > MAX_TAGS {
            bail!("This file has more tags than we can show.");
        }
        if depth > MAX_DEPTH {
            bail!("This file nests tags too deeply to show.");
        }
        Ok(match kind {
            1 => Tag::Byte(self.u8()? as i8),
            2 => Tag::Short(i16::from_be_bytes(self.array()?)),
            3 => Tag::Int(self.i32()?),
            4 => Tag::Long(i64::from_be_bytes(self.array()?).to_string()),
            5 => Tag::Float(f32::from_be_bytes(self.array()?)),
            6 => Tag::Double(f64::from_be_bytes(self.array()?)),
            7 => Tag::ByteArray(self.numbers(|[b]: [u8; 1]| b as i8)?),
            8 => Tag::String(self.string()?),
            9 => {
                let item_kind = self.u8()?;
                let length = self.length()?;
                if length > 0 && item_kind == 0 {
                    bail!("This file has a list of end tags, so it's damaged or isn't NBT.");
                }
                let items = (0..length).map(|_| self.tag(item_kind, depth + 1)).collect::<Result<_>>()?;
                Tag::List(List { item_type: type_name(item_kind), items })
            }
            10 => {
                let mut entries = Vec::new();
                loop {
                    let entry_kind = self.u8()?;
                    if entry_kind == 0 {
                        break;
                    }
                    let name = self.string()?;
                    entries.push((name, self.tag(entry_kind, depth + 1)?));
                }
                Tag::Compound(entries)
            }
            11 => Tag::IntArray(self.numbers(i32::from_be_bytes)?),
            12 => Tag::LongArray(self.numbers(|b| i64::from_be_bytes(b).to_string())?),
            other => bail!("This file has an unknown tag type {other}, so it's damaged or isn't Java Edition NBT."),
        })
    }
}

fn type_name(kind: u8) -> &'static str {
    match kind {
        1 => "byte",
        2 => "short",
        3 => "int",
        4 => "long",
        5 => "float",
        6 => "double",
        7 => "byteArray",
        8 => "string",
        9 => "list",
        10 => "compound",
        11 => "intArray",
        12 => "longArray",
        _ => "end",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn named(kind: u8, name: &str, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![kind];
        out.extend((name.len() as u16).to_be_bytes());
        out.extend(name.as_bytes());
        out.extend(payload);
        out
    }

    fn sample() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend(named(8, "LevelName", &[0, 5, b'W', b'o', b'r', b'l', b'd']));
        body.extend(named(4, "Seed", &(-1234567890123456789i64).to_be_bytes()));
        let mut list = vec![3];
        list.extend(2i32.to_be_bytes());
        list.extend(7i32.to_be_bytes());
        list.extend(8i32.to_be_bytes());
        body.extend(named(9, "Pos", &list));
        let mut longs = 600i32.to_be_bytes().to_vec();
        longs.extend(std::iter::repeat_n(0u8, 600 * 8));
        body.extend(named(12, "States", &longs));
        body.push(0);
        named(10, "", &body)
    }

    #[test]
    fn parses_plain_and_gzipped_nbt() {
        let plain = sample();
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(&plain).unwrap();
        for (bytes, compression) in [(plain.clone(), "none"), (gzip.finish().unwrap(), "gzip")] {
            let document = parse(&bytes).unwrap();
            assert_eq!(document.compression, compression);
            let json = serde_json::to_value(&document.root).unwrap();
            let entries = json["value"].as_array().unwrap();
            assert_eq!(entries[0], serde_json::json!(["LevelName", { "type": "string", "value": "World" }]));
            assert_eq!(entries[1][1]["value"], "-1234567890123456789");
            assert_eq!(entries[2][1]["value"]["itemType"], "int");
            assert_eq!(entries[3][1]["value"]["length"], 600);
            assert_eq!(entries[3][1]["value"]["items"].as_array().unwrap().len(), MAX_ARRAY_ITEMS);
        }
    }

    #[test]
    fn rejects_truncated_and_non_nbt_files() {
        let mut truncated = sample();
        truncated.truncate(20);
        assert!(parse(&truncated).is_err());
        assert!(parse(b"PK\x03\x04 not nbt").is_err());
    }
}
