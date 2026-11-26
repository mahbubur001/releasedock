//! Glob patterns and file classification. Ported from `FileClassifier.swift`.

use crate::text;
use rd_model::{InspectionIssueKind, SourceKind, ValidationSeverity};

/// gitignore-style glob matching used for exclusion patterns and `.distignore`.
///
/// - A pattern without `/` matches any path component (`*.log`, `node_modules`).
/// - A pattern containing `/` is anchored to the package root (`/tests`, `src/*.ts`).
/// - A trailing `/` restricts the pattern to directories.
/// - Matching a directory excludes everything beneath it.
/// - `#` comments, blank lines and `!` negations are ignored (negation is not supported
///   and is reported by the caller if needed).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GlobPattern {
    pub raw: String,
    pattern: String,
    anchored: bool,
    directory_only: bool,
}

impl GlobPattern {
    pub fn new(raw: &str) -> Option<Self> {
        let trimmed = text::trim_whitespace(raw);
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
            return None;
        }
        let directory_only = trimmed.ends_with('/');
        let mut p = trimmed;
        if directory_only {
            p = &p[..p.len() - 1];
        }
        let anchored = p.contains('/');
        if let Some(rest) = p.strip_prefix('/') {
            p = rest;
        }
        if p.is_empty() {
            return None;
        }
        Some(Self {
            raw: raw.to_owned(),
            pattern: p.to_owned(),
            anchored,
            directory_only,
        })
    }

    /// Whether `path` (relative, `/`-separated) is matched directly — not via an ancestor.
    pub fn matches(&self, path: &str, is_directory: bool) -> bool {
        if self.directory_only && !is_directory {
            return false;
        }
        if self.anchored {
            return Self::fnmatch(&self.pattern, path, true);
        }
        let name = path.split('/').rfind(|c| !c.is_empty()).unwrap_or(path);
        Self::fnmatch(&self.pattern, name, false)
    }

    fn fnmatch(pattern: &str, string: &str, pathname: bool) -> bool {
        // `**/` is approximated by trying the pattern with and without the prefix.
        if let Some(rest) = pattern.strip_prefix("**/") {
            if Self::fnmatch(rest, string, pathname) {
                return true;
            }
            let components: Vec<&str> = string.split('/').filter(|c| !c.is_empty()).collect();
            for i in 1..components.len().max(1) {
                if Self::fnmatch(rest, &components[i..].join("/"), pathname) {
                    return true;
                }
            }
            return false;
        }
        let pattern: Vec<char> = pattern.chars().collect();
        let string: Vec<char> = string.chars().collect();
        fnmatch(&pattern, &string, pathname)
    }

    pub fn parse_list(text: &str) -> Vec<GlobPattern> {
        text::lines(text).filter_map(GlobPattern::new).collect()
    }
}

/// POSIX `fnmatch(3)` as implemented by Darwin/FreeBSD libc, with flags `0` or
/// `FNM_PATHNAME` (backslash escapes enabled, no `FNM_PERIOD`), working on characters.
fn fnmatch(pattern: &[char], string: &[char], pathname: bool) -> bool {
    let (mut p, mut s) = (0usize, 0usize);
    loop {
        let Some(&c) = pattern.get(p) else {
            return s == string.len();
        };
        p += 1;
        match c {
            '?' => {
                match string.get(s) {
                    None => return false,
                    Some('/') if pathname => return false,
                    Some(_) => {}
                }
                s += 1;
            }
            '*' => {
                // Collapse multiple stars.
                while pattern.get(p) == Some(&'*') {
                    p += 1;
                }
                let rest = &string[s..];
                match pattern.get(p) {
                    None => return !pathname || !rest.contains(&'/'),
                    Some('/') if pathname => match rest.iter().position(|&ch| ch == '/') {
                        Some(slash) => {
                            s += slash;
                            continue;
                        }
                        None => return false,
                    },
                    Some(_) => {}
                }
                // General case: try every split point.
                let mut t = s;
                while let Some(&test) = string.get(t) {
                    if fnmatch(&pattern[p..], &string[t..], pathname) {
                        return true;
                    }
                    if test == '/' && pathname {
                        break;
                    }
                    t += 1;
                }
                return false;
            }
            '[' => {
                let Some(&test) = string.get(s) else { return false };
                if test == '/' && pathname {
                    return false;
                }
                match range_match(&pattern[p..], test, pathname) {
                    RangeMatch::Match(consumed) => {
                        p += consumed;
                        s += 1;
                    }
                    RangeMatch::NoMatch => return false,
                    // An unterminated bracket is an ordinary character.
                    RangeMatch::Error => {
                        if test != '[' {
                            return false;
                        }
                        s += 1;
                    }
                }
            }
            _ => {
                let literal = if c == '\\' {
                    match pattern.get(p) {
                        Some(&escaped) => {
                            p += 1;
                            escaped
                        }
                        None => '\\',
                    }
                } else {
                    c
                };
                if string.get(s) != Some(&literal) {
                    return false;
                }
                s += 1;
            }
        }
    }
}

enum RangeMatch {
    /// Matched; the bracket expression used this many pattern characters after `[`.
    Match(usize),
    NoMatch,
    Error,
}

fn range_match(pattern: &[char], test: char, pathname: bool) -> RangeMatch {
    let mut p = 0usize;
    let negate = matches!(pattern.first(), Some('!' | '^'));
    if negate {
        p += 1;
    }
    let mut ok = false;
    // The first character may be a literal ']'.
    let mut c = match pattern.get(p) {
        Some(&c) => c,
        None => return RangeMatch::Error,
    };
    p += 1;
    loop {
        if c == '\\' {
            match pattern.get(p) {
                Some(&escaped) => {
                    c = escaped;
                    p += 1;
                }
                None => return RangeMatch::Error,
            }
        }
        if c == '/' && pathname {
            return RangeMatch::NoMatch;
        }
        match (pattern.get(p), pattern.get(p + 1)) {
            (Some('-'), Some(&end)) if end != ']' => {
                p += 2;
                let mut end = end;
                if end == '\\' {
                    match pattern.get(p) {
                        Some(&escaped) => {
                            end = escaped;
                            p += 1;
                        }
                        None => return RangeMatch::Error,
                    }
                }
                if c <= test && test <= end {
                    ok = true;
                }
            }
            _ => {
                if c == test {
                    ok = true;
                }
            }
        }
        match pattern.get(p) {
            None => return RangeMatch::Error,
            Some(']') => {
                p += 1;
                break;
            }
            Some(&next) => {
                c = next;
                p += 1;
            }
        }
    }
    if ok == negate {
        RangeMatch::NoMatch
    } else {
        RangeMatch::Match(p)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FileClassification {
    pub kind: InspectionIssueKind,
    pub severity: ValidationSeverity,
    pub message: String,
    /// Excluded from the release candidate automatically.
    pub exclude: bool,
}

/// Classifies package paths into junk, VCS metadata, development artifacts,
/// forbidden files and possible secrets.
#[derive(Clone, Debug)]
pub struct FileClassifier {
    pub user_patterns: Vec<GlobPattern>,
    pub source_kind: SourceKind,
}

impl FileClassifier {
    pub const JUNK_NAMES: &[&str] = &[
        ".DS_Store",
        "__MACOSX",
        "Thumbs.db",
        "ehthumbs.db",
        "desktop.ini",
        ".Spotlight-V100",
        ".Trashes",
        ".fseventsd",
        ".TemporaryItems",
        "Icon\r",
    ];
    pub const VCS_NAMES: &[&str] = &[
        ".git",
        ".svn",
        ".hg",
        ".bzr",
        "CVS",
        ".gitignore",
        ".gitattributes",
        ".gitmodules",
        ".gitkeep",
        ".svnignore",
        ".hgignore",
    ];
    pub const DEV_NAMES: &[&str] = &[
        "node_modules",
        "bower_components",
        ".github",
        ".gitlab",
        ".gitlab-ci.yml",
        ".circleci",
        ".travis.yml",
        ".wordpress-org",
        ".idea",
        ".vscode",
        ".fleet",
        ".editorconfig",
        ".distignore",
        ".husky",
        ".phpcs.xml",
        ".phpcs.xml.dist",
        "phpcs.xml",
        "phpcs.xml.dist",
        "phpunit.xml",
        "phpunit.xml.dist",
        ".phpunit.result.cache",
        ".phpunit.cache",
        "phpstan.neon",
        "phpstan.neon.dist",
        "psalm.xml",
        ".php-cs-fixer.php",
        ".php-cs-fixer.dist.php",
        ".php-cs-fixer.cache",
        ".php_cs",
        ".php_cs.cache",
        ".eslintignore",
        ".prettierignore",
        ".stylelintignore",
        ".browserslistrc",
        ".babelrc",
        ".nvmrc",
        ".npmrc",
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "Gruntfile.js",
        "gulpfile.js",
        "webpack.config.js",
        ".wp-env.json",
        ".wp-env.override.json",
        "docker-compose.yml",
        "docker-compose.yaml",
        "Dockerfile",
        ".dockerignore",
        ".sass-cache",
        ".phpstorm.meta.php",
        ".env.example",
        ".env.dist",
        ".yarn",
        ".pnp.cjs",
        ".pnp.loader.mjs",
    ];
    pub const DEV_PREFIXES: &[&str] = &[".eslintrc", ".prettierrc", ".stylelintrc", ".yarnrc"];
    pub const DEV_SUFFIXES: &[&str] = &[".log", ".code-workspace", ".swp", ".swo", "~", ".orig", ".rej"];
    pub const ARCHIVE_SUFFIXES: &[&str] = &[".zip", ".tar", ".tar.gz", ".tgz", ".gz", ".bz2", ".xz", ".rar", ".7z"];
    pub const BINARY_SUFFIXES: &[&str] = &[".exe", ".dll", ".dmg", ".pkg", ".msi", ".so", ".dylib", ".jar"];
    pub const SECRET_NAMES: &[&str] = &[
        ".env",
        ".htpasswd",
        "auth.json",
        "wp-config.php",
        "id_rsa",
        "id_dsa",
        "id_ecdsa",
        "id_ed25519",
        ".npmrc.local",
    ];
    pub const SECRET_SUFFIXES: &[&str] = &[".pem", ".key", ".p12", ".pfx", ".keystore", ".jks", ".asc"];

    pub fn new(source_kind: SourceKind, user_patterns: Vec<GlobPattern>) -> Self {
        Self {
            user_patterns,
            source_kind,
        }
    }

    /// True for operating-system metadata (`.DS_Store`, `._*`, …).
    pub fn is_junk_name(name: &str) -> bool {
        Self::JUNK_NAMES.contains(&name) || name.starts_with("._")
    }

    pub fn classify(&self, path: &str, is_directory: bool) -> Option<FileClassification> {
        let name = path.split('/').rfind(|c| !c.is_empty()).unwrap_or(path);
        let lower = name.to_lowercase();
        let classification = |kind, severity, message: &str, exclude| {
            Some(FileClassification {
                kind,
                severity,
                message: message.to_owned(),
                exclude,
            })
        };
        let has_suffix = |suffixes: &[&str]| suffixes.iter().any(|s| lower.ends_with(s));

        if Self::is_junk_name(name) {
            return classification(
                InspectionIssueKind::JunkFile,
                ValidationSeverity::Info,
                "Operating system metadata",
                true,
            );
        }
        if Self::VCS_NAMES.contains(&name) {
            return classification(
                InspectionIssueKind::VersionControlMetadata,
                ValidationSeverity::Warning,
                "Version control metadata",
                true,
            );
        }
        if Self::SECRET_NAMES.contains(&lower.as_str())
            || (lower.starts_with(".env.") && !Self::DEV_NAMES.contains(&name))
            || has_suffix(Self::SECRET_SUFFIXES)
        {
            // In a built ZIP a secret-looking file is a red flag and blocks the release; in a
            // development folder it is expected and simply excluded.
            return classification(
                InspectionIssueKind::PossibleSecret,
                if self.source_kind == SourceKind::Zip {
                    ValidationSeverity::Error
                } else {
                    ValidationSeverity::Warning
                },
                "Possible secret or private key; it must never be published",
                self.source_kind == SourceKind::Folder,
            );
        }
        if Self::DEV_NAMES.contains(&name)
            || Self::DEV_PREFIXES.iter().any(|p| name.starts_with(p))
            || (!is_directory && has_suffix(Self::DEV_SUFFIXES))
        {
            return classification(
                InspectionIssueKind::DevelopmentArtifact,
                ValidationSeverity::Warning,
                "Development artifact",
                true,
            );
        }
        if let Some(pattern) = self.user_patterns.iter().find(|p| p.matches(path, is_directory)) {
            return classification(
                InspectionIssueKind::DevelopmentArtifact,
                ValidationSeverity::Info,
                &format!("Excluded by pattern \"{}\"", pattern.raw),
                true,
            );
        }
        if !is_directory && has_suffix(Self::ARCHIVE_SUFFIXES) {
            return classification(
                InspectionIssueKind::ForbiddenFile,
                ValidationSeverity::Error,
                "Archive files must not be committed to WordPress.org SVN",
                false,
            );
        }
        if !is_directory && has_suffix(Self::BINARY_SUFFIXES) {
            return classification(
                InspectionIssueKind::ForbiddenFile,
                ValidationSeverity::Error,
                "Executable or compiled binary files are not allowed in a release",
                false,
            );
        }
        if !is_directory && lower.ends_with(".phar") {
            return classification(
                InspectionIssueKind::ForbiddenFile,
                ValidationSeverity::Warning,
                "PHAR archives are usually rejected by WordPress.org review",
                false,
            );
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pattern: &str, string: &str, pathname: bool) -> bool {
        GlobPattern::fnmatch(pattern, string, pathname)
    }

    #[test]
    fn fnmatch_follows_libc() {
        assert!(m("*.php", "a.php", false));
        assert!(m("*.php", "a/b.php", false), "without FNM_PATHNAME * crosses /");
        assert!(!m("*.php", "a/b.php", true));
        assert!(m("src/*.ts", "src/app.ts", true));
        assert!(!m("src/*.ts", "src/x/app.ts", true));
        assert!(m("src/*", "src/app.ts", true));
        assert!(!m("src/*", "src/a/b", true));
        assert!(m("a?c", "abc", true));
        assert!(!m("a?c", "a/c", true));
        assert!(m("[abc].txt", "b.txt", false));
        assert!(m("[!abc].txt", "d.txt", false));
        assert!(m("[^abc].txt", "d.txt", false));
        assert!(!m("[!abc].txt", "a.txt", false));
        assert!(m("[a-c]x", "bx", false));
        assert!(m("[]]", "]", false));
        assert!(m("[a-]", "-", false));
        assert!(m("[", "[", false), "an unterminated bracket is literal");
        assert!(m("\\*", "*", false));
        assert!(!m("\\*", "a", false));
        assert!(m("*/x", "a/x", true));
        assert!(!m("*/x", "a/b/x", true));
        assert!(m("a**b", "axxb", true));
    }

    #[test]
    fn pattern_parsing() {
        assert_eq!(GlobPattern::new("/"), None);
        assert_eq!(GlobPattern::new("   "), None);
        let p = GlobPattern::new("  /vendor/ ").unwrap();
        assert_eq!(p.raw, "  /vendor/ ");
        assert!(p.matches("vendor", true));
        assert!(!p.matches("vendor", false));
        assert!(!p.matches("x/vendor", true));
        assert_eq!(GlobPattern::parse_list("a\r\nb\n#c\n!d\n").len(), 2);
    }
}
