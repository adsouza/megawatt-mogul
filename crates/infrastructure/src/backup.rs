//! Version-1 ZIP transport for existing format-4 archives. No filesystem or gameplay mutation.
use crate::save::{EncodedArchive, MAX_SAVE_BYTES, decode_archive, replay_compatibility};
use application::SaveError;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Write},
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};
pub const BACKUP_VERSION: u32 = 1;
pub const MAX_BACKUP_BYTES: usize = 2 * MAX_SAVE_BYTES + 2 * 1024 * 1024;
const COMMENT: &str = "megawatt-mogul-backup:1";
const MAX_ENTRIES: usize = 4097;
fn malformed(error: impl std::fmt::Display) -> SaveError {
    SaveError::Malformed(error.to_string())
}
fn u16_at(bytes: &[u8], at: usize) -> Result<u16, SaveError> {
    let data = bytes
        .get(at..at + 2)
        .ok_or_else(|| malformed("truncated ZIP header"))?;
    Ok(u16::from_le_bytes([data[0], data[1]]))
}
fn u32_at(bytes: &[u8], at: usize) -> Result<u32, SaveError> {
    let data = bytes
        .get(at..at + 4)
        .ok_or_else(|| malformed("truncated ZIP header"))?;
    Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}
/// Bound and validate the central directory before the ZIP library can allocate its metadata.
fn preflight(bytes: &[u8]) -> Result<usize, SaveError> {
    if bytes.len() > MAX_BACKUP_BYTES {
        return Err(SaveError::SaveTooLarge);
    }
    let start = bytes.len().saturating_sub(65557);
    let end = (start..bytes.len().saturating_sub(21))
        .rev()
        .find(|&at| {
            bytes.get(at..at + 4) == Some(b"PK\x05\x06")
                && u16_at(bytes, at + 20).is_ok_and(|n| at + 22 + usize::from(n) == bytes.len())
        })
        .ok_or_else(|| malformed("missing ZIP directory"))?;
    let comment = std::str::from_utf8(&bytes[end + 22..]).map_err(malformed)?;
    let version = comment
        .strip_prefix("megawatt-mogul-backup:")
        .ok_or_else(|| malformed("missing backup version"))?
        .parse::<u32>()
        .map_err(malformed)?;
    if version != BACKUP_VERSION {
        return Err(SaveError::SaveFormatVersionMismatch {
            recorded: version,
            supported: BACKUP_VERSION,
        });
    }
    let count = usize::from(u16_at(bytes, end + 10)?);
    if count == 0 || count > MAX_ENTRIES {
        return Err(SaveError::SaveTooLarge);
    }
    if u16_at(bytes, end + 4)? != 0
        || u16_at(bytes, end + 6)? != 0
        || usize::from(u16_at(bytes, end + 8)?) != count
    {
        return Err(malformed("split ZIP is unsupported"));
    }
    let mut at = u32_at(bytes, end + 16)? as usize;
    let directory_len = u32_at(bytes, end + 12)? as usize;
    if at.checked_add(directory_len) != Some(end) {
        return Err(malformed("invalid ZIP directory bounds"));
    }
    let mut names = BTreeSet::new();
    let mut expanded = 0usize;
    for _ in 0..count {
        if bytes.get(at..at + 4) != Some(b"PK\x01\x02") {
            return Err(malformed("invalid central ZIP header"));
        }
        let flags = u16_at(bytes, at + 8)?;
        let method = u16_at(bytes, at + 10)?;
        if flags & 1 != 0 || !matches!(method, 0 | 8) {
            return Err(malformed("encrypted or unsupported ZIP entry"));
        }
        let length = u32_at(bytes, at + 24)? as usize;
        expanded = expanded
            .checked_add(length)
            .ok_or(SaveError::SaveTooLarge)?;
        if length > MAX_SAVE_BYTES
            || expanded > 2 * MAX_SAVE_BYTES
            || u32_at(bytes, at + 20)? as usize > MAX_SAVE_BYTES
        {
            return Err(SaveError::SaveTooLarge);
        }
        let name_len = usize::from(u16_at(bytes, at + 28)?);
        let extra = usize::from(u16_at(bytes, at + 30)?);
        let comment = usize::from(u16_at(bytes, at + 32)?);
        let next = at
            .checked_add(46 + name_len + extra + comment)
            .filter(|&n| n <= end)
            .ok_or_else(|| malformed("ZIP entry bounds"))?;
        let name = bytes
            .get(at + 46..at + 46 + name_len)
            .ok_or_else(|| malformed("ZIP entry name"))?;
        if name.is_empty()
            || name.starts_with(b".")
            || name.windows(2).any(|w| w == b"..")
            || !name
                .iter()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'.'))
        {
            return Err(malformed("invalid backup entry name"));
        }
        if !names.insert(name) {
            return Err(malformed("duplicate backup entry"));
        }
        if u16_at(bytes, at + 34)? != 0 {
            return Err(malformed("split ZIP entry"));
        }
        let attributes = u32_at(bytes, at + 38)?;
        let mode = (attributes >> 16) & 0o170000;
        if attributes & 0x10 != 0 || !matches!(mode, 0 | 0o100000) {
            return Err(malformed("non-file backup entry"));
        }
        if u32_at(bytes, at + 42)? as usize >= end - directory_len {
            return Err(malformed("invalid local ZIP offset"));
        }
        at = next;
    }
    if at != end {
        return Err(malformed("unexpected ZIP directory data"));
    }
    Ok(count)
}
pub fn encode_backup(archive: &EncodedArchive) -> Result<Vec<u8>, SaveError> {
    decode_archive(archive, &replay_compatibility(), &|| false)?;
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.set_comment(COMMENT).map_err(malformed)?;
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(zip::DateTime::default());
    for (name, bytes) in std::iter::once(("snapshot.ron.z", archive.manifest.as_slice())).chain(
        archive
            .files
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice())),
    ) {
        writer.start_file(name, options).map_err(malformed)?;
        writer.write_all(bytes).map_err(malformed)?;
    }
    let output = writer.finish().map_err(malformed)?.into_inner();
    if output.len() > MAX_BACKUP_BYTES {
        return Err(SaveError::SaveTooLarge);
    }
    Ok(output)
}
pub fn decode_backup(bytes: &[u8]) -> Result<EncodedArchive, SaveError> {
    let count = preflight(bytes)?;
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(malformed)?;
    if zip.len() != count {
        return Err(malformed("ZIP entry count differs"));
    }
    let mut files = BTreeMap::new();
    let mut manifest = None;
    for index in 0..count {
        let file = zip.by_index(index).map_err(malformed)?;
        if file.encrypted()
            || file.is_dir()
            || !matches!(
                file.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            )
        {
            return Err(malformed("unsupported ZIP entry"));
        }
        let name = std::str::from_utf8(file.name_raw())
            .map_err(malformed)?
            .to_owned();
        let expected = file.size();
        let mut data = Vec::new();
        file.take(MAX_SAVE_BYTES as u64 + 1)
            .read_to_end(&mut data)
            .map_err(malformed)?;
        if data.len() > MAX_SAVE_BYTES {
            return Err(SaveError::SaveTooLarge);
        }
        if data.len() as u64 != expected {
            return Err(SaveError::ChecksumMismatch);
        }
        if name == "snapshot.ron.z" {
            manifest = Some(data);
        } else {
            files.insert(name, data);
        }
    }
    let archive = EncodedArchive {
        manifest: manifest.ok_or_else(|| malformed("missing snapshot manifest"))?,
        files,
    };
    decode_archive(&archive, &replay_compatibility(), &|| false)?;
    Ok(archive)
}
