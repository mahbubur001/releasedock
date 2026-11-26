//! The few `FileManager` / `URL` operations the pipeline needs.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// `createDirectory(withIntermediateDirectories: true, attributes: [.posixPermissions: mode])`.
/// The mode applies on Unix only.
#[cfg(unix)]
pub(crate) fn create_dirs(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().recursive(true).mode(mode).create(path)
}

#[cfg(not(unix))]
pub(crate) fn create_dirs(path: &Path, _mode: u32) -> io::Result<()> {
    fs::create_dir_all(path)
}

/// `FileManager.removeItem(at:)`: a file, a symbolic link (not its target) or a directory tree.
pub(crate) fn remove_item(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        remove_file_or_link(path)
    }
}

#[cfg(windows)]
fn remove_file_or_link(path: &Path) -> io::Result<()> {
    // A directory symlink on Windows is removed with remove_dir.
    fs::remove_file(path).or_else(|_| fs::remove_dir(path))
}

#[cfg(not(windows))]
fn remove_file_or_link(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

/// `FileManager.fileExists(atPath:)`, which follows symbolic links.
pub(crate) fn file_exists(path: &Path) -> bool {
    fs::metadata(path).is_ok()
}

/// `fileExists(atPath:isDirectory:)`: (exists, is a directory), following symbolic links.
pub(crate) fn file_exists_is_dir(path: &Path) -> (bool, bool) {
    match fs::metadata(path) {
        Ok(metadata) => (true, metadata.is_dir()),
        Err(_) => (false, false),
    }
}

/// `URL.standardizedFileURL`: removes `.` and resolves `..` lexically (no symlink resolution).
pub(crate) fn standardized(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other),
        }
    }
    result
}

/// The part of `path` (as svn reports it) below `root`, with `/` separators. svn reports
/// paths as given on the command line, which on Windows may use either separator.
pub(crate) fn relative_to_root(path: &str, root: &str) -> Option<String> {
    let root = root.trim_end_matches(['/', '\\']);
    let rest = strip_prefix_platform(path, root)?;
    let rest = rest.strip_prefix('/').or_else(|| rest.strip_prefix('\\'))?;
    Some(rest.replace('\\', "/"))
}

#[cfg(windows)]
fn strip_prefix_platform<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    let normalize = |s: &str| s.replace('\\', "/").to_lowercase();
    if path.len() >= root.len()
        && path.is_char_boundary(root.len())
        && normalize(&path[..root.len()]) == normalize(root)
    {
        Some(&path[root.len()..])
    } else {
        None
    }
}

#[cfg(not(windows))]
fn strip_prefix_platform<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    path.strip_prefix(root)
}

/// `Data.write(to:options: .atomic)`: a temporary file in the same folder, then a rename.
pub(crate) fn write_atomically(path: &Path, data: &[u8]) -> io::Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temporary = path.with_file_name(format!(".{name}.{}.tmp", rd_model::Id::new()));
    fs::write(&temporary, data)?;
    fs::rename(&temporary, path).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })
}

/// A path as the string stored in records (`URL.path`).
pub(crate) fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Swift `trimmingTrailingSlash`: drops one trailing `/`.
pub(crate) fn trimming_trailing_slash(text: &str) -> &str {
    text.strip_suffix('/').unwrap_or(text)
}

/// `NSString.pathExtension` of the last path component: text after the last `.`, empty
/// when the name has no `.` other than a leading one.
pub(crate) fn path_extension(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(index) if index > 0 => &name[index + 1..],
        _ => "",
    }
}

/// `URL.isFileURL` for a URL string.
pub(crate) fn is_file_url(url: &str) -> bool {
    url.split_once(':')
        .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case("file"))
}

/// `URL(string:)?.host` for an absolute URL string (`scheme://[user@]host[:port]/…`).
pub(crate) fn url_host(url: &str) -> Option<&str> {
    let (scheme, rest) = url.split_once("://")?;
    let mut chars = scheme.chars();
    if !chars.next()?.is_ascii_alphabetic() || !chars.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) {
        return None;
    }
    if url.chars().any(char::is_whitespace) {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if let Some(bracketed) = host_port.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or("")
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    (!host.is_empty()).then_some(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_below_the_root() {
        assert_eq!(relative_to_root("/wc/trunk/a.php", "/wc"), Some("trunk/a.php".into()));
        assert_eq!(relative_to_root("/wc/trunk/a.php", "/wc/"), Some("trunk/a.php".into()));
        assert_eq!(relative_to_root("/wc", "/wc"), None);
        assert_eq!(relative_to_root("/wcx/a", "/wc"), None);
    }

    #[test]
    fn path_extensions_like_nsstring() {
        assert_eq!(path_extension("banner-772x250.PNG"), "PNG");
        assert_eq!(path_extension("dir/icon.svg"), "svg");
        assert_eq!(path_extension(".htaccess"), "");
        assert_eq!(path_extension("README"), "");
    }

    #[test]
    fn hosts_and_file_urls() {
        assert_eq!(
            url_host("https://plugins.svn.wordpress.org/acme/"),
            Some("plugins.svn.wordpress.org")
        );
        assert_eq!(url_host("file:///tmp/repo"), None);
        assert!(is_file_url("file:///tmp/repo"));
        assert!(!is_file_url("https://plugins.svn.wordpress.org/acme"));
        assert_eq!(trimming_trailing_slash("https://x/y/"), "https://x/y");
        assert_eq!(trimming_trailing_slash("https://x/y"), "https://x/y");
    }
}
