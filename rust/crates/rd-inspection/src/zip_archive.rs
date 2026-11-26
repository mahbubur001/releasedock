//! Minimal, defensive ZIP reader. Ported from `ZIPArchive.swift`.

use crate::error::Error;
use flate2::{Decompress, FlushDecompress, Status};
use rd_support::hashing::Crc32;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ZipError {
    #[error("ZIP file is too large ({0} bytes)")]
    FileTooLarge(u64),
    #[error("The file is not a ZIP archive")]
    NotAZip,
    #[error("Corrupt ZIP archive: {0}")]
    Corrupt(String),
    #[error("Unsupported ZIP feature: {0}")]
    Unsupported(String),
    #[error("Entry exceeds its declared size: {0}")]
    EntryTooLarge(String),
    #[error("CRC-32 mismatch for {0}")]
    ChecksumMismatch(String),
    #[error("Refusing to overwrite existing file: {0}")]
    DestinationExists(String),
}

fn corrupt(detail: impl Into<String>) -> ZipError {
    ZipError::Corrupt(detail.into())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ZipEntry {
    pub raw_name: Vec<u8>,
    /// Decoded name, or `None` when the name is not valid UTF-8.
    pub name: Option<String>,
    pub flags: u16,
    pub method: u16,
    pub crc32: u32,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub local_header_offset: u64,
    pub version_made_by: u16,
    pub external_attributes: u32,
}

impl ZipEntry {
    pub fn is_encrypted(&self) -> bool {
        self.flags & 0x0001 != 0
    }

    /// Unix mode bits when the archive was created on a Unix host.
    pub fn unix_mode(&self) -> Option<u32> {
        if self.version_made_by >> 8 != 3 {
            return None;
        }
        let mode = self.external_attributes >> 16;
        if mode == 0 { None } else { Some(mode) }
    }

    pub fn is_symlink(&self) -> bool {
        self.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000)
    }

    pub fn is_directory(&self) -> bool {
        if self.name.as_deref().is_some_and(|n| n.ends_with('/')) {
            return true;
        }
        if let Some(mode) = self.unix_mode() {
            return mode & 0o170000 == 0o040000;
        }
        self.external_attributes & 0x10 != 0
    }

    /// FIFOs, devices, sockets — anything that is not a file, directory or symlink.
    pub fn is_special_file(&self) -> bool {
        let Some(mode) = self.unix_mode() else { return false };
        let kind = mode & 0o170000;
        kind != 0o100000 && kind != 0o040000 && kind != 0o120000
    }

    fn label(&self) -> String {
        self.name.clone().unwrap_or_else(|| "<binary name>".to_owned())
    }
}

/// Minimal, defensive ZIP reader. It parses the central directory itself (including
/// ZIP64), validates every structure against the file bounds, cross-checks local
/// headers, and supports only the *stored* and *deflate* methods. Nothing is written
/// to disk except through [`ZipArchive::extract`], which never overwrites or follows links.
#[derive(Debug)]
pub struct ZipArchive {
    pub path: PathBuf,
    pub entries: Vec<ZipEntry>,
    data: Vec<u8>,
}

impl ZipArchive {
    pub const MAXIMUM_ARCHIVE_SIZE: u64 = 1 << 30; // 1 GiB

    /// Opens and validates an archive. Structural problems are [`Error::Zip`]; failing to
    /// read the file is [`Error::Io`].
    pub fn open(path: &Path) -> Result<Self, Error> {
        let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        if size > Self::MAXIMUM_ARCHIVE_SIZE {
            return Err(ZipError::FileTooLarge(size).into());
        }
        let data = fs::read(path)?;
        let entries = read_central_directory(&data)?;
        Ok(Self {
            path: path.to_path_buf(),
            entries,
            data,
        })
    }

    /// Validates an in-memory archive.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, ZipError> {
        if data.len() as u64 > Self::MAXIMUM_ARCHIVE_SIZE {
            return Err(ZipError::FileTooLarge(data.len() as u64));
        }
        let entries = read_central_directory(&data)?;
        Ok(Self {
            path: PathBuf::new(),
            entries,
            data,
        })
    }

    // MARK: Extraction

    /// Streams the entry's decompressed bytes to `sink`, enforcing the declared size and CRC-32.
    pub fn read(&self, entry: &ZipEntry, mut sink: impl FnMut(&[u8]) -> Result<(), Error>) -> Result<(), Error> {
        if entry.is_encrypted() {
            return Err(ZipError::Unsupported("encrypted entries".into()).into());
        }
        let label = entry.label();
        let (data_start, _) = local_header(&self.data, entry)?;
        let length = int(entry.compressed_size, "compressed size")?;
        let compressed = data_start
            .checked_add(length)
            .and_then(|end| self.data.get(data_start..end))
            .ok_or_else(|| corrupt("entry data outside file bounds"))?;
        let mut crc = Crc32::new();
        let mut produced: u64 = 0;

        let mut emit = |chunk: &[u8]| -> Result<(), Error> {
            produced += chunk.len() as u64;
            if produced > entry.uncompressed_size {
                return Err(ZipError::EntryTooLarge(label.clone()).into());
            }
            crc.update(chunk);
            sink(chunk)
        };

        match entry.method {
            0 => {
                if entry.compressed_size != entry.uncompressed_size {
                    return Err(corrupt(format!("stored entry size mismatch for {label}")).into());
                }
                for chunk in compressed.chunks(1 << 20) {
                    emit(chunk)?;
                }
            }
            8 => {
                // Raw DEFLATE (RFC 1951, no zlib header), which is what ZIP stores.
                let mut inflater = Decompress::new(false);
                let mut output = vec![0u8; 1 << 16];
                let mut input = compressed;
                loop {
                    let (before_in, before_out) = (inflater.total_in(), inflater.total_out());
                    let status = inflater
                        .decompress(input, &mut output, FlushDecompress::None)
                        .map_err(|_| corrupt(format!("invalid deflate data for {label}")))?;
                    let consumed = (inflater.total_in() - before_in) as usize;
                    let written = (inflater.total_out() - before_out) as usize;
                    input = input.get(consumed..).unwrap_or_default();
                    if written > 0 {
                        emit(output.get(..written).unwrap_or_default())?;
                    }
                    if status == Status::StreamEnd || (consumed == 0 && written == 0) {
                        break;
                    }
                }
            }
            method => {
                return Err(ZipError::Unsupported(format!("compression method {method} for {label}")).into());
            }
        }

        if produced != entry.uncompressed_size {
            return Err(corrupt(format!("size mismatch for {label}")).into());
        }
        if crc.checksum() != entry.crc32 {
            return Err(ZipError::ChecksumMismatch(label).into());
        }
        Ok(())
    }

    /// Writes a regular-file entry to `destination`. The destination must not exist; the
    /// file is created exclusively (`O_CREAT|O_EXCL`, which never follows a symlink at the
    /// final component) with mode 0644. Returns the SHA-256 of the written content.
    pub fn extract(&self, entry: &ZipEntry, destination: &Path) -> Result<String, Error> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o644);
        }
        let mut file = match options.open(destination) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                return Err(ZipError::DestinationExists(destination.display().to_string()).into());
            }
            Err(e) => return Err(e.into()),
        };
        let mut hasher = Sha256::new();
        let result = self
            .read(entry, |chunk| {
                hasher.update(chunk);
                file.write_all(chunk)?;
                Ok(())
            })
            .and_then(|()| file.flush().map_err(Error::from));
        drop(file);
        if let Err(error) = result {
            let _ = fs::remove_file(destination);
            return Err(error);
        }
        Ok(hex::encode(hasher.finalize()))
    }
}

// MARK: Reading primitives

fn u16_at(d: &[u8], offset: usize) -> Result<u16, ZipError> {
    match offset.checked_add(2).and_then(|end| d.get(offset..end)) {
        Some(b) => Ok(u16::from(b[0]) | u16::from(b[1]) << 8),
        None => Err(corrupt("read past end of file")),
    }
}

fn u32_at(d: &[u8], offset: usize) -> Result<u32, ZipError> {
    match offset.checked_add(4).and_then(|end| d.get(offset..end)) {
        Some(b) => Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
        None => Err(corrupt("read past end of file")),
    }
}

fn u64_at(d: &[u8], offset: usize) -> Result<u64, ZipError> {
    let low = u64::from(u32_at(d, offset)?);
    let high = u64::from(u32_at(
        d,
        offset.checked_add(4).ok_or_else(|| corrupt("read past end of file"))?,
    )?);
    Ok(low | high << 32)
}

/// Converts an untrusted offset or size. Nothing in a valid archive can exceed the
/// archive size limit, and bounding values here keeps later additions overflow-free.
fn int(value: u64, what: &str) -> Result<usize, ZipError> {
    if value > ZipArchive::MAXIMUM_ARCHIVE_SIZE {
        return Err(corrupt(format!("{what} out of range")));
    }
    Ok(value as usize)
}

// MARK: Central directory

pub(crate) fn read_central_directory(d: &[u8]) -> Result<Vec<ZipEntry>, ZipError> {
    if d.len() < 22 {
        return Err(ZipError::NotAZip);
    }
    // End of central directory: search backwards over the maximum comment length.
    let search_start = d.len().saturating_sub(22 + 0xFFFF);
    let mut eocd = None;
    let mut i = d.len() - 22;
    loop {
        if u32_at(d, i)? == 0x0605_4B50 {
            // The comment length must reach exactly to the end of the file.
            let comment_length = usize::from(u16_at(d, i + 20)?);
            if i + 22 + comment_length == d.len() {
                eocd = Some(i);
                break;
            }
        }
        if i == search_start {
            break;
        }
        i -= 1;
    }
    let Some(eocd) = eocd else {
        return Err(ZipError::NotAZip);
    };

    let disk_number = u16_at(d, eocd + 4)?;
    let cd_disk = u16_at(d, eocd + 6)?;
    let mut total_entries = u64::from(u16_at(d, eocd + 10)?);
    let mut cd_size = u64::from(u32_at(d, eocd + 12)?);
    let mut cd_offset = u64::from(u32_at(d, eocd + 16)?);
    if disk_number != 0 || cd_disk != 0 {
        return Err(ZipError::Unsupported("multi-disk archives".into()));
    }

    // ZIP64 end of central directory.
    if total_entries == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF {
        let locator = eocd.checked_sub(20);
        let Some(locator) = locator.filter(|&l| u32_at(d, l).is_ok_and(|s| s == 0x0706_4B50)) else {
            return Err(corrupt("missing ZIP64 end of central directory locator"));
        };
        let z64 = int(u64_at(d, locator + 8)?, "ZIP64 record offset")?;
        if u32_at(d, z64)? != 0x0606_4B50 {
            return Err(corrupt("invalid ZIP64 end of central directory"));
        }
        total_entries = u64_at(d, z64 + 32)?;
        cd_size = u64_at(d, z64 + 40)?;
        cd_offset = u64_at(d, z64 + 48)?;
    }

    let cd_start = int(cd_offset, "central directory offset")?;
    let cd_length = int(cd_size, "central directory size")?;
    let cd_end = cd_start + cd_length;
    if cd_end > eocd {
        return Err(corrupt("central directory outside file bounds"));
    }
    if total_entries > 100_000 {
        return Err(ZipError::Unsupported("more than 100000 entries".into()));
    }

    let mut entries = Vec::with_capacity(total_entries as usize);
    let mut p = cd_start;
    for _ in 0..total_entries {
        if u32_at(d, p)? != 0x0201_4B50 {
            return Err(corrupt("invalid central directory header"));
        }
        let version_made_by = u16_at(d, p + 4)?;
        let flags = u16_at(d, p + 8)?;
        let method = u16_at(d, p + 10)?;
        let crc = u32_at(d, p + 16)?;
        let mut compressed = u64::from(u32_at(d, p + 20)?);
        let mut uncompressed = u64::from(u32_at(d, p + 24)?);
        let name_length = usize::from(u16_at(d, p + 28)?);
        let extra_length = usize::from(u16_at(d, p + 30)?);
        let comment_length = usize::from(u16_at(d, p + 32)?);
        let disk_start = u16_at(d, p + 34)?;
        let external = u32_at(d, p + 38)?;
        let mut local_offset = u64::from(u32_at(d, p + 42)?);
        let name_start = p + 46;
        let extra_start = name_start + name_length;
        let extra_end = extra_start + extra_length;
        let next = extra_end + comment_length;
        if next > cd_end {
            return Err(corrupt("central directory entry overruns directory"));
        }

        if disk_start != 0 && disk_start != 0xFFFF {
            return Err(ZipError::Unsupported("multi-disk archives".into()));
        }
        let raw_name = d
            .get(name_start..extra_start)
            .ok_or_else(|| corrupt("read past end of file"))?
            .to_vec();

        // ZIP64 extended information extra field.
        let mut e = extra_start;
        while e + 4 <= extra_end {
            let header_id = u16_at(d, e)?;
            let size = usize::from(u16_at(d, e + 2)?);
            if e + 4 + size > extra_end {
                return Err(corrupt("extra field overruns entry"));
            }
            if header_id == 0x0001 {
                let mut f = e + 4;
                if uncompressed == 0xFFFF_FFFF {
                    uncompressed = u64_at(d, f)?;
                    f += 8;
                }
                if compressed == 0xFFFF_FFFF {
                    compressed = u64_at(d, f)?;
                    f += 8;
                }
                if local_offset == 0xFFFF_FFFF {
                    local_offset = u64_at(d, f)?;
                    f += 8;
                }
                if f > e + 4 + size {
                    return Err(corrupt("truncated ZIP64 extra field"));
                }
            }
            e += 4 + size;
        }

        let utf8_flag = flags & 0x0800 != 0;
        let mut name = String::from_utf8(raw_name.clone()).ok();
        // Without the UTF-8 flag, names are CP437 by spec; ASCII is identical in both,
        // and macOS tools write UTF-8 without setting the flag. Non-UTF-8 stays None.
        if name.is_none() && !utf8_flag && raw_name.is_ascii() {
            name = Some(String::from_utf8_lossy(&raw_name).into_owned());
        }

        entries.push(ZipEntry {
            raw_name,
            name,
            flags,
            method,
            crc32: crc,
            compressed_size: compressed,
            uncompressed_size: uncompressed,
            local_header_offset: local_offset,
            version_made_by,
            external_attributes: external,
        });
        p = next;
    }

    validate_layout(d, &entries, cd_start)?;
    Ok(entries)
}

/// Ensures local headers exist, agree with the central directory, lie before the
/// central directory and do not overlap (overlapping entries are a zip-bomb technique).
fn validate_layout(d: &[u8], entries: &[ZipEntry], central_directory_start: usize) -> Result<(), ZipError> {
    let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(entries.len());
    for entry in entries {
        let (data_start, local_name) = local_header(d, entry)?;
        if local_name != entry.raw_name.as_slice() {
            return Err(corrupt(format!(
                "local file name differs from central directory for {}",
                entry.label()
            )));
        }
        let end = data_start + int(entry.compressed_size, "compressed size")?;
        if end > central_directory_start {
            return Err(corrupt("entry data overlaps central directory"));
        }
        ranges.push((int(entry.local_header_offset, "local header offset")?, end));
    }
    ranges.sort_by_key(|r| r.0);
    if ranges.windows(2).any(|pair| pair[1].0 < pair[0].1) {
        return Err(corrupt("overlapping entries"));
    }
    Ok(())
}

/// Returns the offset of the entry's data and the name stored in the local header.
fn local_header<'a>(d: &'a [u8], entry: &ZipEntry) -> Result<(usize, &'a [u8]), ZipError> {
    let offset = int(entry.local_header_offset, "local header offset")?;
    if u32_at(d, offset)? != 0x0403_4B50 {
        return Err(corrupt("invalid local file header"));
    }
    let name_length = usize::from(u16_at(d, offset + 26)?);
    let extra_length = usize::from(u16_at(d, offset + 28)?);
    let name_start = offset + 30;
    let data_start = name_start + name_length + extra_length;
    if data_start > d.len() {
        return Err(corrupt("local header overruns file"));
    }
    let name = d
        .get(name_start..name_start + name_length)
        .ok_or_else(|| corrupt("local header overruns file"))?;
    Ok((data_start, name))
}

#[cfg(test)]
mod tests {
    //! Reader-level checks beyond the ported Swift tests: ZIP64 records, deflate bombs
    //! that lie about their size, overlapping entries and CRC errors.
    use super::*;
    use flate2::Compression;
    use flate2::write::DeflateEncoder;

    struct Raw {
        name: &'static [u8],
        method: u16,
        stored: Vec<u8>,
        crc: u32,
        uncompressed: u32,
    }

    fn le16(out: &mut Vec<u8>, v: u16) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    fn le32(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    /// Writes entries; `offsets` overrides the local header offset in the central directory.
    fn build(entries: &[Raw], offsets: Option<&[u32]>) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (i, e) in entries.iter().enumerate() {
            let offset = offsets.map_or(out.len() as u32, |o| o[i]);
            if offsets.is_none() || i == 0 {
                le32(&mut out, 0x0403_4B50);
                for v in [20, 0x0800, e.method, 0, 0] {
                    le16(&mut out, v);
                }
                le32(&mut out, e.crc);
                le32(&mut out, e.stored.len() as u32);
                le32(&mut out, e.uncompressed);
                le16(&mut out, e.name.len() as u16);
                le16(&mut out, 0);
                out.extend_from_slice(e.name);
                out.extend_from_slice(&e.stored);
            }
            le32(&mut central, 0x0201_4B50);
            for v in [0x031E, 20, 0x0800, e.method, 0, 0] {
                le16(&mut central, v);
            }
            le32(&mut central, e.crc);
            le32(&mut central, e.stored.len() as u32);
            le32(&mut central, e.uncompressed);
            for v in [e.name.len() as u16, 0, 0, 0, 0] {
                le16(&mut central, v);
            }
            le32(&mut central, 0o100644 << 16);
            le32(&mut central, offset);
            central.extend_from_slice(e.name);
        }
        let cd_offset = out.len() as u32;
        let cd_size = central.len() as u32;
        out.extend_from_slice(&central);
        le32(&mut out, 0x0605_4B50);
        for v in [0, 0, entries.len() as u16, entries.len() as u16] {
            le16(&mut out, v);
        }
        le32(&mut out, cd_size);
        le32(&mut out, cd_offset);
        le16(&mut out, 0);
        out
    }

    fn deflate(data: &[u8]) -> Vec<u8> {
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn read_all(archive: &ZipArchive, entry: &ZipEntry) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        archive.read(entry, |chunk| {
            out.extend_from_slice(chunk);
            Ok(())
        })?;
        Ok(out)
    }

    #[test]
    fn reads_deflate_and_stored_entries() {
        let text = b"hello hello hello hello".repeat(100);
        let data = build(
            &[
                Raw {
                    name: b"a.txt",
                    method: 8,
                    stored: deflate(&text),
                    crc: Crc32::of(&text),
                    uncompressed: text.len() as u32,
                },
                Raw {
                    name: b"b.txt",
                    method: 0,
                    stored: b"plain".to_vec(),
                    crc: Crc32::of(b"plain"),
                    uncompressed: 5,
                },
            ],
            None,
        );
        let archive = ZipArchive::from_bytes(data).unwrap();
        assert_eq!(read_all(&archive, &archive.entries[0]).unwrap(), text);
        assert_eq!(read_all(&archive, &archive.entries[1]).unwrap(), b"plain");
    }

    #[test]
    fn deflate_bomb_that_understates_its_size_stops_early() {
        let text = vec![0u8; 1 << 20];
        let data = build(
            &[Raw {
                name: b"bomb.txt",
                method: 8,
                stored: deflate(&text),
                crc: Crc32::of(&text),
                uncompressed: 1000,
            }],
            None,
        );
        let archive = ZipArchive::from_bytes(data).unwrap();
        let mut seen = 0usize;
        let result = archive.read(&archive.entries[0], |chunk| {
            seen += chunk.len();
            Ok(())
        });
        assert!(
            matches!(result, Err(Error::Zip(ZipError::EntryTooLarge(_)))),
            "{result:?}"
        );
        assert!(seen <= 1000);
    }

    #[test]
    fn crc_mismatch_and_overlap_are_detected() {
        let bad_crc = build(
            &[Raw {
                name: b"a.txt",
                method: 0,
                stored: b"abc".to_vec(),
                crc: 1,
                uncompressed: 3,
            }],
            None,
        );
        let archive = ZipArchive::from_bytes(bad_crc).unwrap();
        assert!(matches!(
            read_all(&archive, &archive.entries[0]),
            Err(Error::Zip(ZipError::ChecksumMismatch(_)))
        ));

        let entry = || Raw {
            name: b"a.txt",
            method: 0,
            stored: b"abc".to_vec(),
            crc: Crc32::of(b"abc"),
            uncompressed: 3,
        };
        // Two central directory records pointing at the same local header.
        let overlapping = build(&[entry(), entry()], Some(&[0, 0]));
        assert_eq!(
            ZipArchive::from_bytes(overlapping).unwrap_err(),
            ZipError::Corrupt("overlapping entries".into())
        );
    }

    #[test]
    fn reads_zip64_records_and_extra_fields() {
        // Local header + data.
        let mut out = Vec::new();
        le32(&mut out, 0x0403_4B50);
        for v in [45, 0x0800, 0, 0, 0] {
            le16(&mut out, v);
        }
        le32(&mut out, Crc32::of(b"abc"));
        le32(&mut out, 0xFFFF_FFFF);
        le32(&mut out, 0xFFFF_FFFF);
        le16(&mut out, 5);
        le16(&mut out, 0);
        out.extend_from_slice(b"a.txt");
        out.extend_from_slice(b"abc");
        // Central directory with every size and the offset in the ZIP64 extra field.
        let cd_offset = out.len() as u64;
        let mut central = Vec::new();
        le32(&mut central, 0x0201_4B50);
        for v in [0x032D, 45, 0x0800, 0, 0, 0] {
            le16(&mut central, v);
        }
        le32(&mut central, Crc32::of(b"abc"));
        le32(&mut central, 0xFFFF_FFFF);
        le32(&mut central, 0xFFFF_FFFF);
        for v in [5, 28, 0, 0xFFFF, 0] {
            le16(&mut central, v);
        }
        le32(&mut central, 0o100644 << 16);
        le32(&mut central, 0xFFFF_FFFF);
        central.extend_from_slice(b"a.txt");
        le16(&mut central, 0x0001);
        le16(&mut central, 24);
        central.extend_from_slice(&3u64.to_le_bytes());
        central.extend_from_slice(&3u64.to_le_bytes());
        central.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&central);
        // ZIP64 end of central directory record and locator.
        let z64 = out.len() as u64;
        le32(&mut out, 0x0606_4B50);
        out.extend_from_slice(&44u64.to_le_bytes());
        for v in [45, 45] {
            le16(&mut out, v);
        }
        le32(&mut out, 0);
        le32(&mut out, 0);
        out.extend_from_slice(&1u64.to_le_bytes());
        out.extend_from_slice(&1u64.to_le_bytes());
        out.extend_from_slice(&(central.len() as u64).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        le32(&mut out, 0x0706_4B50);
        le32(&mut out, 0);
        out.extend_from_slice(&z64.to_le_bytes());
        le32(&mut out, 1);
        le32(&mut out, 0x0605_4B50);
        for v in [0, 0, 0xFFFF, 0xFFFF] {
            le16(&mut out, v);
        }
        le32(&mut out, 0xFFFF_FFFF);
        le32(&mut out, 0xFFFF_FFFF);
        le16(&mut out, 0);

        let archive = ZipArchive::from_bytes(out).unwrap();
        assert_eq!(archive.entries.len(), 1);
        assert_eq!(archive.entries[0].uncompressed_size, 3);
        assert_eq!(archive.entries[0].local_header_offset, 0);
        assert_eq!(read_all(&archive, &archive.entries[0]).unwrap(), b"abc");
    }

    #[test]
    fn rejects_truncated_and_non_zip_data() {
        assert_eq!(ZipArchive::from_bytes(vec![0; 10]).unwrap_err(), ZipError::NotAZip);
        assert_eq!(ZipArchive::from_bytes(vec![0; 100]).unwrap_err(), ZipError::NotAZip);
        let mut data = build(
            &[Raw {
                name: b"a.txt",
                method: 0,
                stored: b"abc".to_vec(),
                crc: Crc32::of(b"abc"),
                uncompressed: 3,
            }],
            None,
        );
        // Claim a central directory larger than the space before the EOCD.
        let eocd = data.len() - 22;
        data[eocd + 12..eocd + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        assert_eq!(
            ZipArchive::from_bytes(data).unwrap_err(),
            ZipError::Corrupt("central directory outside file bounds".into())
        );
    }
}
