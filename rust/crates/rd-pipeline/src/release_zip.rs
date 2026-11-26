//! Writes the GitHub release asset: the staged candidate as `<slug>/…` in a deflate ZIP.
//! Swift runs `/usr/bin/ditto -c -k --norsrc --noextattr --noqtn --keepParent`, which only
//! exists on macOS; this writer produces the same layout on every platform (directory
//! entries, deflated files, UTF-8 names, Unix modes, modification times) without resource
//! forks or extended attributes.

use flate2::Compression;
use flate2::write::DeflateEncoder;
use rd_support::hashing::Crc32;
use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Zips the tree at `source` as `parent/…` into `destination` (replaced if present).
/// Symbolic links are not followed and not stored; candidates never contain them.
pub fn zip_tree(source: &Path, parent: &str, destination: &Path) -> io::Result<()> {
    let mut items = Vec::new();
    collect(source, parent, &mut items)?;
    let temporary = destination.with_extension("zip.partial");
    let result = write_archive(&items, &temporary).and_then(|()| fs::rename(&temporary, destination));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

struct Item {
    name: String,
    path: std::path::PathBuf,
    is_dir: bool,
}

fn collect(path: &Path, name: &str, items: &mut Vec<Item>) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        items.push(Item {
            name: format!("{name}/"),
            path: path.to_path_buf(),
            is_dir: true,
        });
        let mut children: Vec<_> = fs::read_dir(path)?.collect::<Result<_, _>>()?;
        children.sort_by_key(|c| c.file_name());
        for child in children {
            let child_name = format!("{name}/{}", child.file_name().to_string_lossy());
            collect(&child.path(), &child_name, items)?;
        }
    } else if metadata.is_file() {
        items.push(Item {
            name: name.to_owned(),
            path: path.to_path_buf(),
            is_dir: false,
        });
    }
    Ok(())
}

fn too_large(name: &str) -> io::Error {
    io::Error::other(format!("{name} is too large for a ZIP archive without ZIP64"))
}

fn write_archive(items: &[Item], destination: &Path) -> io::Result<()> {
    let mut out = BufWriter::new(fs::File::create(destination)?);
    let mut central = Vec::new();
    let mut offset: u64 = 0;
    for item in items {
        let metadata = fs::metadata(&item.path)?;
        let (time, date) = dos_date_time(metadata.modified().unwrap_or(UNIX_EPOCH));
        let (method, crc, size, stored, mode) = if item.is_dir {
            (0u16, 0u32, 0u64, Vec::new(), 0o040755u32)
        } else {
            let data = fs::read(&item.path)?;
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(&data)?;
            let compressed = encoder.finish()?;
            (
                8u16,
                Crc32::of(&data),
                data.len() as u64,
                compressed,
                file_mode(&metadata),
            )
        };
        let name = item.name.as_bytes();
        let compressed_size = u32::try_from(stored.len()).map_err(|_| too_large(&item.name))?;
        let size = u32::try_from(size).map_err(|_| too_large(&item.name))?;
        let local_offset = u32::try_from(offset).map_err(|_| too_large(&item.name))?;
        let name_length = u16::try_from(name.len()).map_err(|_| too_large(&item.name))?;

        let mut local = Vec::with_capacity(30 + name.len());
        local.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
        for v in [20u16, 0x0800, method, time, date] {
            local.extend_from_slice(&v.to_le_bytes());
        }
        for v in [crc, compressed_size, size] {
            local.extend_from_slice(&v.to_le_bytes());
        }
        local.extend_from_slice(&name_length.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(name);
        out.write_all(&local)?;
        out.write_all(&stored)?;
        offset += (local.len() + stored.len()) as u64;

        central.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
        for v in [0x031Eu16, 20, 0x0800, method, time, date] {
            central.extend_from_slice(&v.to_le_bytes());
        }
        for v in [crc, compressed_size, size] {
            central.extend_from_slice(&v.to_le_bytes());
        }
        for v in [name_length, 0, 0, 0, 0] {
            central.extend_from_slice(&v.to_le_bytes());
        }
        central.extend_from_slice(&(mode << 16).to_le_bytes());
        central.extend_from_slice(&local_offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let count = u16::try_from(items.len()).map_err(|_| io::Error::other("Too many files for a ZIP archive"))?;
    let central_offset = u32::try_from(offset).map_err(|_| too_large("The archive"))?;
    let central_size = u32::try_from(central.len()).map_err(|_| too_large("The archive"))?;
    out.write_all(&central)?;
    let mut end = Vec::with_capacity(22);
    end.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
    for v in [0u16, 0, count, count] {
        end.extend_from_slice(&v.to_le_bytes());
    }
    end.extend_from_slice(&central_size.to_le_bytes());
    end.extend_from_slice(&central_offset.to_le_bytes());
    end.extend_from_slice(&0u16.to_le_bytes());
    out.write_all(&end)?;
    out.flush()
}

#[cfg(unix)]
fn file_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    0o100000 | (metadata.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn file_mode(_metadata: &fs::Metadata) -> u32 {
    0o100644
}

/// MS-DOS time and date (UTC; DOS has no time zone) of `time`, clamped to 1980–2107.
fn dos_date_time(time: SystemTime) -> (u16, u16) {
    let seconds = time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let days = seconds.div_euclid(86_400);
    let remainder = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    if year < 1980 {
        return (0, (1 << 5) | 1);
    }
    let year = year.min(2107);
    let time = ((remainder / 3600) << 11) | (((remainder % 3600) / 60) << 5) | ((remainder % 60) / 2);
    let date = ((year - 1980) << 9) | (month << 5) | day;
    (time as u16, date as u16)
}

/// Days since 1970-01-01 to (year, month, day) in the proleptic Gregorian calendar.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_783), (2026, 11, 26));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }

    #[test]
    fn dos_time_of_a_known_instant() {
        // 2026-11-26 09:30:16 UTC
        let time = UNIX_EPOCH + std::time::Duration::from_secs(20_783 * 86_400 + 9 * 3600 + 30 * 60 + 16);
        let (t, d) = dos_date_time(time);
        assert_eq!(t, (9 << 11) | (30 << 5) | 8);
        assert_eq!(d, ((2026 - 1980) << 9) | (11 << 5) | 26);
    }
}
