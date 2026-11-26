//! SHA-256 of files, bytes and file trees, and CRC-32 for ZIP entries.

use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// Streams a file through SHA-256 without loading it fully into memory.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn sha256(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// Deterministic digest of a file tree from (relative path, file digest) pairs, sorted by
/// path. Used as the identity of a folder source and of a release preview.
pub fn tree_digest<'a>(entries: impl IntoIterator<Item = (&'a str, &'a str)>) -> String {
    let mut entries: Vec<_> = entries.into_iter().collect();
    // Swift sorts Strings by Unicode scalar values; byte order of UTF-8 is the same order.
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let mut hasher = Sha256::new();
    for (path, digest) in entries {
        hasher.update(path.as_bytes());
        hasher.update([0u8]);
        hasher.update(digest.as_bytes());
        hasher.update([0x0Au8]);
    }
    hex::encode(hasher.finalize())
}

/// CRC-32 (IEEE 802.3), used to verify ZIP entry integrity.
#[derive(Clone, Copy, Debug)]
pub struct Crc32 {
    value: u32,
}

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

static TABLE: [u32; 256] = crc_table();

impl Default for Crc32 {
    fn default() -> Self {
        Self { value: 0xFFFF_FFFF }
    }
}

impl Crc32 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, data: &[u8]) {
        let mut c = self.value;
        for byte in data {
            c = TABLE[((c ^ u32::from(*byte)) & 0xFF) as usize] ^ (c >> 8);
        }
        self.value = c;
    }

    pub fn checksum(&self) -> u32 {
        self.value ^ 0xFFFF_FFFF
    }

    pub fn of(data: &[u8]) -> u32 {
        let mut crc = Self::new();
        crc.update(data);
        crc.checksum()
    }
}
