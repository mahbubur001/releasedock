//! Updates the version everywhere it is written in a development folder, so the header,
//! `Stable tag`, version constants and `package.json` never drift apart. This is the only
//! place ReleaseDock writes to a development folder, and only after the user approves the
//! planned lines.

use crate::detector::ProductDetector;
use crate::php_version;
use crate::text::{split_lf_characters, trim_newlines, trim_whitespace, trim_whitespace_and_newlines};
use crate::version_service::{VersionService, VersionServicing, VersionValidity};
use chrono::{DateTime, Local};
use rd_model::{Product, ProductType, SwiftDate};
use rd_support::hashing;
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// One line the bump changes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionBumpChange {
    pub file: String,
    /// 1-based line number, or 0 for an inserted changelog entry.
    pub line: usize,
    pub description: String,
    pub old_text: String,
    pub new_text: String,
}

impl VersionBumpChange {
    pub fn id(&self) -> String {
        format!("{}:{}", self.file, self.line)
    }
}

/// Everything a version bump will write, computed without touching the folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionBumpPlan {
    pub current_version: String,
    pub new_version: String,
    pub changes: Vec<VersionBumpChange>,
    /// Things the user should know (no changelog entry, version not newer…).
    pub notes: Vec<String>,
    /// SHA-256 of each file as planned; applying refuses if any changed meanwhile.
    pub file_hashes: BTreeMap<String, String>,
    /// New contents per file.
    pub(crate) contents: BTreeMap<String, String>,
}

impl VersionBumpPlan {
    /// The changed files, in the order of their first change.
    pub fn files(&self) -> Vec<String> {
        let mut files: Vec<String> = Vec::new();
        for change in &self.changes {
            if !files.contains(&change.file) {
                files.push(change.file.clone());
            }
        }
        files
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VersionBumpError {
    #[error("No plugin or theme found in the folder: {0}")]
    NoProduct(String),
    #[error("\"{0}\" is not a valid version number.")]
    InvalidVersion(String),
    #[error("{new} is not newer than the current version {current}.")]
    NotNewer { current: String, new: String },
    #[error("Nothing to change: no version references were found.")]
    NothingToChange,
    #[error("{0} changed after the preview. Review the bump again.")]
    FileChanged(String),
    /// Reading or writing a file failed (Swift rethrows the Foundation error).
    #[error("{0}")]
    Io(String),
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> VersionBumpError + '_ {
    move |error| VersionBumpError::Io(format!("{}: {error}", path.display()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Part {
    Major,
    Minor,
    Patch,
}

impl Part {
    pub const ALL: [Part; 3] = [Part::Major, Part::Minor, Part::Patch];

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Major => "major",
            Self::Minor => "minor",
            Self::Patch => "patch",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct VersionBumpService;

impl VersionBumpService {
    pub fn new() -> Self {
        Self
    }

    /// `2.3.9` → patch `2.3.10`, minor `2.4.0`, major `3.0.0`. Pre-release suffixes are dropped.
    /// Returns None instead of overflowing (Swift would trap).
    pub fn suggest(part: Part, version: &str) -> Option<String> {
        let core = version.split('-').find(|s| !s.is_empty()).unwrap_or(version);
        let pieces: Vec<&str> = core.split('.').filter(|s| !s.is_empty()).collect();
        let mut numbers: Vec<i64> = pieces.iter().filter_map(|p| p.parse().ok()).collect();
        if numbers.is_empty() || numbers.len() != pieces.len() {
            return None;
        }
        while numbers.len() < 3 {
            numbers.push(0);
        }
        numbers = match part {
            Part::Major => vec![numbers[0].checked_add(1)?, 0, 0],
            Part::Minor => vec![numbers[0], numbers[1].checked_add(1)?, 0],
            Part::Patch => {
                numbers[2] = numbers[2].checked_add(1)?;
                numbers
            }
        };
        Some(numbers.iter().take(3).map(i64::to_string).collect::<Vec<_>>().join("."))
    }

    /// Computes the bump without writing. `today` dates the changelog entry in the local
    /// time zone (pass `SwiftDate::now()`).
    pub fn plan(
        &self,
        folder: &Path,
        product: &Product,
        new_version: &str,
        changelog_notes: &str,
        today: SwiftDate,
    ) -> Result<VersionBumpPlan, VersionBumpError> {
        let new_version = trim_whitespace(new_version);
        if !matches!(VersionService.validate(new_version), VersionValidity::Valid { .. }) {
            return Err(VersionBumpError::InvalidVersion(new_version.into()));
        }
        let detected = ProductDetector
            .detect(
                folder,
                Some(product.product_type),
                product.main_plugin_file.as_deref(),
                Some(&product.slug),
            )
            .map_err(|error| VersionBumpError::NoProduct(error.to_string()))?;
        let (Some(main_file), Some(current)) = (detected.main_file.clone(), detected.version.clone()) else {
            return Err(VersionBumpError::NoProduct(
                detected
                    .problems
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "no Version header".into()),
            ));
        };
        if php_version::compare(new_version, &current) <= 0 {
            return Err(VersionBumpError::NotNewer {
                current,
                new: new_version.into(),
            });
        }

        let mut editor = Editor::new(folder);
        // 1. The header WordPress reads.
        editor.replace_header(&main_file, "Version", &current, new_version)?;
        // 2. Stable tag (plugins), so the readme points at the new tag.
        if let Some(readme) = &detected.readme_file {
            let parsed = detected.readme.as_ref();
            if product.product_type == ProductType::Plugin && parsed.and_then(|r| r.stable_tag()).is_some() {
                editor.replace_readme_field(readme, "Stable tag", new_version)?;
            }
            // 3. Changelog entry, only from notes the user typed.
            let notes: Vec<&str> = split_lf_characters(changelog_notes)
                .into_iter()
                .map(trim_whitespace)
                .filter(|n| !n.is_empty())
                .collect();
            if !notes.is_empty() {
                editor.insert_changelog(readme, new_version, &notes, today)?;
            } else if !parsed.is_some_and(|r| r.changelog_mentions(new_version)) {
                editor.notes.push(format!(
                    "readme.txt has no changelog entry for {new_version}. Add notes here, or edit the readme yourself."
                ));
            }
        }
        // 4. Version constants in PHP files that still carry the current version.
        for file in self.php_files(folder) {
            editor.replace_constants(&file, &current, new_version)?;
        }
        // 5. package.json.
        if folder.join("package.json").exists() {
            editor.replace_package_json(&current, new_version)?;
        }

        if editor.changes.is_empty() {
            return Err(VersionBumpError::NothingToChange);
        }
        Ok(VersionBumpPlan {
            current_version: current,
            new_version: new_version.into(),
            changes: editor.changes,
            notes: editor.notes,
            file_hashes: editor.original_hashes,
            contents: editor.contents,
        })
    }

    /// Writes the planned contents. Refuses when a file changed since planning.
    pub fn apply(&self, plan: &VersionBumpPlan, folder: &Path) -> Result<(), VersionBumpError> {
        let files = plan.files();
        for file in &files {
            let path = folder.join(file);
            let current = hashing::sha256_file(&path).map_err(io_error(&path))?;
            if plan.file_hashes.get(file) != Some(&current) {
                return Err(VersionBumpError::FileChanged(file.clone()));
            }
        }
        for file in &files {
            let Some(text) = plan.contents.get(file) else {
                continue;
            };
            let path = folder.join(file);
            write_atomic(&path, text.as_bytes()).map_err(io_error(&path))?;
        }
        Ok(())
    }

    /// PHP files that can hold version constants; dependency and build folders are skipped.
    /// Paths are relative and `/`-separated.
    pub fn php_files(&self, folder: &Path) -> Vec<String> {
        fn walk(dir: &Path, prefix: &str, files: &mut Vec<String>) {
            const SKIP: [&str; 6] = ["vendor", "node_modules", ".git", "tests", "build", "dist"];
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                // `.skipsHiddenFiles`: hidden entries are neither listed nor descended into.
                if name.starts_with('.') || SKIP.contains(&name.as_str()) {
                    continue;
                }
                let relative = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                if Path::new(&name)
                    .extension()
                    .is_some_and(|ext| ext.to_string_lossy().to_lowercase() == "php")
                {
                    files.push(relative.clone());
                }
                // Like FileManager's enumerator: symbolic links are listed but not followed.
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    walk(&entry.path(), &relative, files);
                }
            }
        }
        let mut files = Vec::new();
        walk(folder, "", &mut files);
        files.sort();
        files
    }
}

/// Replaces the file through a temporary file in the same directory, keeping its permissions
/// (as `Data.write(options: .atomic)` does).
fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(data)?;
    temp.as_file().sync_all()?;
    if let Ok(metadata) = fs::metadata(path) {
        fs::set_permissions(temp.path(), metadata.permissions())?;
    }
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// `Calendar.current` year-month-day of `date`.
fn local_day(date: SwiftDate) -> String {
    let unix = date.unix_seconds();
    let seconds = unix.floor();
    let nanos = ((unix - seconds) * 1e9) as u32;
    match DateTime::from_timestamp(seconds as i64, nanos) {
        Some(utc) => utc.with_timezone(&Local).format("%Y-%m-%d").to_string(),
        None => "0000-00-00".into(),
    }
}

/// Swift `NSRegularExpression` `firstMatch` + `stringByReplacingMatches`: None when nothing
/// matches, otherwise the line with every match replaced. `replacement` returns None for a
/// candidate the ICU pattern would reject (emulated back-references), which is left as is.
fn replacing(line: &str, regex: &Regex, replacement: impl Fn(&Captures) -> Option<String>) -> Option<String> {
    let mut output = String::with_capacity(line.len());
    let mut last = 0;
    let mut matched = false;
    for caps in regex.captures_iter(line) {
        let Some(whole) = caps.get(0) else { continue };
        let Some(replaced) = replacement(&caps) else { continue };
        matched = true;
        output.push_str(&line[last..whole.start()]);
        output.push_str(&replaced);
        last = whole.end();
    }
    if !matched {
        return None;
    }
    output.push_str(&line[last..]);
    Some(output)
}

fn group<'a>(caps: &'a Captures, index: usize) -> &'a str {
    caps.get(index).map_or("", |m| m.as_str())
}

/// ICU's `.`: any character except a line terminator.
const ICU_DOT: &str = r"[^\n\x0B\x0C\r\x{85}\x{2028}\x{2029}]";

/// Accumulates per-file text edits, keeping each file's line endings.
struct Editor<'a> {
    folder: &'a Path,
    contents: BTreeMap<String, String>,
    original_hashes: BTreeMap<String, String>,
    changes: Vec<VersionBumpChange>,
    notes: Vec<String>,
}

impl<'a> Editor<'a> {
    fn new(folder: &'a Path) -> Self {
        Self {
            folder,
            contents: BTreeMap::new(),
            original_hashes: BTreeMap::new(),
            changes: Vec::new(),
            notes: Vec::new(),
        }
    }

    fn text(&mut self, file: &str) -> Result<String, VersionBumpError> {
        if let Some(text) = self.contents.get(file) {
            return Ok(text.clone());
        }
        let path = self.folder.join(file);
        let data = fs::read(&path).map_err(io_error(&path))?;
        self.original_hashes.insert(file.into(), hashing::sha256(&data));
        let text = String::from_utf8_lossy(&data).into_owned();
        self.contents.insert(file.into(), text.clone());
        Ok(text)
    }

    /// Splits keeping line terminators so writing back preserves CRLF files. Swift splits
    /// after the Characters `\n` and `\r\n`, i.e. after every `\n`.
    fn lines(text: &str) -> Vec<String> {
        text.split_inclusive('\n').map(str::to_string).collect()
    }

    fn edit(
        &mut self,
        file: &str,
        description: &str,
        limit: usize,
        transform: impl Fn(&str) -> Option<String>,
    ) -> Result<(), VersionBumpError> {
        let mut lines = Self::lines(&self.text(file)?);
        let mut edited = 0;
        for (index, line) in lines.iter_mut().enumerate() {
            if edited >= limit {
                break;
            }
            let Some(replaced) = transform(line) else { continue };
            if replaced == *line {
                continue;
            }
            self.changes.push(VersionBumpChange {
                file: file.into(),
                line: index + 1,
                description: description.into(),
                old_text: trim_newlines(line).into(),
                new_text: trim_newlines(&replaced).into(),
            });
            *line = replaced;
            edited += 1;
        }
        self.contents.insert(file.into(), lines.concat());
        Ok(())
    }

    /// The first `Version:` header line in the 8 KiB WordPress reads.
    fn replace_header(&mut self, file: &str, header: &str, old: &str, new: &str) -> Result<(), VersionBumpError> {
        // Swift: `^((?:[ \t]*<\?(?:php)?)?[ \t/*#@]*{header}:[ \t]*){old}(?![0-9A-Za-z.])`.
        // The look-ahead becomes a captured following character (or the end), put back
        // unchanged; the pattern is anchored, so a line has at most one match.
        let pattern = format!(
            r"(?i)^((?:[ \t]*<\?(?:php)?)?[ \t/*#@]*{}:[ \t]*){}([^0-9A-Za-z.]|\z)",
            regex::escape(header),
            regex::escape(old)
        );
        let regex = Regex::new(&pattern).ok();
        let description = format!("{header} header");
        self.edit(file, &description, 1, |line| {
            replacing(line, regex.as_ref()?, |caps| {
                Some(format!("{}{new}{}", group(caps, 1), group(caps, 2)))
            })
        })
    }

    fn replace_readme_field(&mut self, file: &str, field: &str, new: &str) -> Result<(), VersionBumpError> {
        // Swift: `^([ \t*-]*{field}[ \t]*:[ \t]*)\S.*?([ \t]*)$` on the line without its
        // terminator, where `.` is ICU's (no line terminators) and `$` is the end.
        let pattern = format!(
            r"(?i)^([ \t*-]*{}[ \t]*:[ \t]*)\S{ICU_DOT}*?([ \t]*)$",
            regex::escape(field)
        );
        let regex = Regex::new(&pattern).ok();
        self.edit(file, field, 1, |line| {
            let body = trim_newlines(line);
            let ending: String = line.chars().skip(body.chars().count()).collect();
            replacing(body, regex.as_ref()?, |caps| {
                Some(format!("{}{new}{}", group(caps, 1), group(caps, 2)))
            })
            .map(|replaced| replaced + &ending)
        })
    }

    /// `define( 'ACME_VERSION', '2.3.0' )` and `const VERSION = '2.3.0';`
    fn replace_constants(&mut self, file: &str, old: &str, new: &str) -> Result<(), VersionBumpError> {
        // Swift matches the closing quote with the back-reference `\2`; here both quotes are
        // captured and a candidate whose quotes differ is not a match.
        let quoted = format!(r#"(['"]){}(['"])"#, regex::escape(old));
        let define = Regex::new(&format!(
            r#"(?i)(define\s*\(\s*['"][A-Za-z0-9_\\]*VERSION['"]\s*,\s*){quoted}"#
        ))
        .ok();
        let constant = Regex::new(&format!(
            r"(?i)((?:(?:public|protected|private|final)\s+)*const\s+[A-Za-z0-9_]*VERSION\s*=\s*){quoted}"
        ))
        .ok();
        let template = |caps: &Captures| {
            let (quote, closing) = (group(caps, 2), group(caps, 3));
            (quote == closing).then(|| format!("{}{quote}{new}{quote}", group(caps, 1)))
        };
        self.edit(file, "Version constant", usize::MAX, |line| {
            define
                .as_ref()
                .and_then(|r| replacing(line, r, template))
                .or_else(|| constant.as_ref().and_then(|r| replacing(line, r, template)))
        })
    }

    /// The top-level `"version"` of package.json (its first occurrence).
    fn replace_package_json(&mut self, old: &str, new: &str) -> Result<(), VersionBumpError> {
        let pattern = format!(r#"(?i)^(\s*"version"\s*:\s*"){}(")"#, regex::escape(old));
        let regex = Regex::new(&pattern).ok();
        self.edit("package.json", "package.json version", 1, |line| {
            replacing(line, regex.as_ref()?, |caps| {
                Some(format!("{}{new}{}", group(caps, 1), group(caps, 2)))
            })
        })
    }

    /// Inserts `= version - date =` with the notes as bullets right under `== Changelog ==`.
    fn insert_changelog(
        &mut self,
        file: &str,
        version: &str,
        entry_notes: &[&str],
        date: SwiftDate,
    ) -> Result<(), VersionBumpError> {
        let mut lines = Self::lines(&self.text(file)?);
        let Some(heading) = lines.iter().position(|line| {
            let t = trim_whitespace_and_newlines(line).to_lowercase();
            t.starts_with("==") && t.contains("changelog")
        }) else {
            self.notes.push(format!(
                "readme.txt has no == Changelog == section; add the {version} entry yourself."
            ));
            return Ok(());
        };
        let newline = if lines[heading].ends_with("\r\n") { "\r\n" } else { "\n" };
        let day = local_day(date);
        let bullets = entry_notes.iter().map(|note| {
            if note.starts_with('*') || note.starts_with('-') {
                note.to_string()
            } else {
                format!("* {note}")
            }
        });
        let mut block = vec![format!("= {version} - {day} =")];
        block.extend(bullets);
        block.push(String::new());
        let next = heading + 1;
        let with_newlines = |items: &[String]| items.iter().map(|l| format!("{l}{newline}")).collect::<Vec<_>>();
        if next < lines.len() && trim_whitespace_and_newlines(&lines[next]).is_empty() {
            lines.splice(next + 1..next + 1, with_newlines(&block));
        } else {
            let mut padded = vec![String::new()];
            padded.extend(block.iter().cloned());
            lines.splice(next..next, with_newlines(&padded));
        }
        self.contents.insert(file.into(), lines.concat());
        self.changes.push(VersionBumpChange {
            file: file.into(),
            line: 0,
            description: "Changelog entry".into(),
            old_text: String::new(),
            new_text: block[..block.len() - 1].join("\n"),
        });
        Ok(())
    }
}
