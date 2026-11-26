//! Validation of untrusted relative paths (ZIP entry names, folder entries) and safe
//! resolution of those paths beneath a trusted root directory.

use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Violation {
    #[error("Empty path")]
    Empty,
    #[error("Absolute path")]
    AbsolutePath,
    #[error("Path contains '..' traversal")]
    ParentTraversal,
    #[error("Path contains a backslash separator")]
    Backslash,
    #[error("Path contains control characters")]
    NulOrControlCharacter,
    #[error("Path is not valid UTF-8")]
    InvalidUtf8,
    #[error("Path component longer than 255 bytes: {}…", truncate(.0, 40))]
    ComponentTooLong(String),
    #[error("Path longer than 1024 bytes")]
    PathTooLong,
    #[error("Path resolves outside the destination directory")]
    EscapesRoot,
}

fn truncate(text: &str, characters: usize) -> String {
    text.chars().take(characters).collect()
}

/// Characters Windows forbids in file names. WordPress sites run on Windows too, so these
/// are reported as portability warnings.
pub const WINDOWS_RESERVED_CHARACTERS: &[char] = &['<', '>', ':', '"', '|', '?', '*'];

/// Validates an untrusted, `/`-separated relative path and returns its normalized
/// components (empty and `.` components removed).
pub fn validate_relative_path(raw: &str) -> Result<Vec<String>, Violation> {
    if raw.is_empty() {
        return Err(Violation::Empty);
    }
    if raw.len() > 1024 {
        return Err(Violation::PathTooLong);
    }
    if raw.starts_with('/') || raw.starts_with('~') {
        return Err(Violation::AbsolutePath);
    }
    // Drive-letter or UNC style paths from Windows archivers.
    if raw.chars().nth(1) == Some(':') {
        return Err(Violation::AbsolutePath);
    }
    if raw.contains('\\') {
        return Err(Violation::Backslash);
    }
    if raw.chars().any(|c| (c as u32) < 0x20 || c as u32 == 0x7F) {
        return Err(Violation::NulOrControlCharacter);
    }
    let mut components = Vec::new();
    for component in raw.split('/').filter(|c| !c.is_empty()) {
        match component {
            "." => continue,
            ".." => return Err(Violation::ParentTraversal),
            c if c.len() > 255 => return Err(Violation::ComponentTooLong(c.to_owned())),
            c => components.push(c.to_owned()),
        }
    }
    if components.is_empty() {
        return Err(Violation::Empty);
    }
    Ok(components)
}

/// Resolves validated components beneath `root`, verifying the result is still contained
/// by `root` after resolving symbolic links.
pub fn resolve(components: &[String], root: &Path) -> Result<PathBuf, Violation> {
    let mut path = root.to_path_buf();
    for component in components {
        path.push(component);
    }
    if is_contained(&path, root) {
        Ok(path)
    } else {
        Err(Violation::EscapesRoot)
    }
}

/// True when `path` is `root` or lies beneath it after resolving every symbolic link in
/// the existing part of both paths. Paths that cannot be resolved are not contained.
pub fn is_contained(path: &Path, root: &Path) -> bool {
    match (canonical_path(root), canonical_path(path)) {
        (Some(root), Some(candidate)) => candidate == root || candidate.starts_with(&root),
        _ => false,
    }
}

/// Canonical form of the deepest existing ancestor (found with `symlink_metadata`, so a
/// symlink counts as an existing entry and is then resolved), with the non-existing
/// remainder appended.
pub fn canonical_path(path: &Path) -> Option<PathBuf> {
    let mut current = normalize(path);
    let mut tail = Vec::new();
    while fs::symlink_metadata(&current).is_err() {
        let name = current.file_name()?.to_owned();
        tail.push(name);
        current = current.parent()?.to_path_buf();
    }
    let mut resolved = fs::canonicalize(&current).ok()?;
    for component in tail.into_iter().rev() {
        resolved.push(component);
    }
    Some(resolved)
}

/// Removes `.` and resolves `..` lexically, like `URL.standardizedFileURL`.
fn normalize(path: &Path) -> PathBuf {
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

/// A user-entered slug: lowercase letters, digits and hyphens only, matching
/// WordPress.org directory slugs.
pub fn is_valid_slug(slug: &str) -> bool {
    if slug.is_empty() || slug.chars().count() > 200 || slug.starts_with('-') || slug.ends_with('-') {
        return false;
    }
    slug.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A portability problem for a single file-name component, if any.
pub fn portability_warning(name: &str) -> Option<&'static str> {
    if name.contains(WINDOWS_RESERVED_CHARACTERS) {
        return Some("Contains characters that are invalid on Windows (< > : \" | ? *)");
    }
    if name != name.trim_matches(|c: char| c == ' ' || c == '\t') {
        return Some("Has leading or trailing whitespace");
    }
    if name.ends_with('.') {
        return Some("Ends with a period, which Windows strips");
    }
    const RESERVED: [&str; 11] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "LPT1", "LPT2", "LPT3",
    ];
    let stem = name.split('.').next().unwrap_or("").to_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return Some("Uses a reserved Windows device name");
    }
    None
}
