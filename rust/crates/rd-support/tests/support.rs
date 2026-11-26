//! Ported from InspectionTests.swift (PathSafetyTests), ReleaseNotesTests.swift and
//! CommitTemplateTests.swift.

use rd_support::path_safety::{self, Violation};
use rd_support::{commit_template, hashing, redact::redact, release_notes::*};

#[test]
fn rejects_unsafe_paths() {
    let long = "x".repeat(256);
    let bad = [
        ("", Violation::Empty),
        ("/etc/passwd", Violation::AbsolutePath),
        ("~/x", Violation::AbsolutePath),
        ("C:/Windows", Violation::AbsolutePath),
        ("a/../../b", Violation::ParentTraversal),
        ("..", Violation::ParentTraversal),
        ("a\\b", Violation::Backslash),
        ("a\0b", Violation::NulOrControlCharacter),
        ("a\nb", Violation::NulOrControlCharacter),
        (long.as_str(), Violation::ComponentTooLong(long.clone())),
    ];
    for (path, violation) in bad {
        assert_eq!(path_safety::validate_relative_path(path), Err(violation), "{path:?}");
    }
}

#[test]
fn normalizes_safe_paths() {
    assert_eq!(
        path_safety::validate_relative_path("a/./b//c.php").unwrap(),
        ["a", "b", "c.php"]
    );
    assert_eq!(
        path_safety::validate_relative_path("logo@2x.png").unwrap(),
        ["logo@2x.png"]
    );
}

#[cfg(unix)]
#[test]
fn containment_resolves_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(dir.path().join("outside")).unwrap();
    std::os::unix::fs::symlink(dir.path().join("outside"), root.join("link")).unwrap();
    assert!(path_safety::is_contained(&root.join("a/b"), &root));
    assert!(!path_safety::is_contained(&root.join("link/x"), &root));
    assert!(!path_safety::is_contained(&dir.path().join("rootless"), &root));
}

#[test]
fn slugs() {
    assert!(path_safety::is_valid_slug("my-plugin-2"));
    for bad in ["My-Plugin", "-x", "a/b", "../x"] {
        assert!(!path_safety::is_valid_slug(bad), "{bad}");
    }
}

#[test]
fn portability_warnings() {
    assert!(path_safety::portability_warning("a:b.php").is_some());
    assert!(path_safety::portability_warning("con.php").is_some());
    assert!(path_safety::portability_warning("name.").is_some());
    assert!(path_safety::portability_warning(" lead.php").is_some());
    assert_eq!(path_safety::portability_warning("readme.txt"), None);
}

#[test]
fn redactor() {
    assert_eq!(redact("svn --password hunter2 x"), "svn --password <redacted> x");
    assert_eq!(
        redact("https://bob:secret@example.com/x"),
        "https://bob:<redacted>@example.com/x"
    );
    assert!(!redact("token ghp_abcdefghijklmnopqrstuvwxyz0123").contains("abcdefghijklmnop"));
    assert_eq!(
        redact("Authorization: Bearer abc.def"),
        "Authorization: Bearer <redacted>"
    );
}

#[test]
fn hashes_match_known_values() {
    assert_eq!(
        hashing::sha256(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(hashing::Crc32::of(b"123456789"), 0xCBF4_3926);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.txt");
    std::fs::write(&file, b"abc").unwrap();
    assert_eq!(hashing::sha256_file(&file).unwrap(), hashing::sha256(b"abc"));
    // Order-independent and sensitive to both path and digest.
    let a = hashing::tree_digest([("b.php", "2"), ("a.php", "1")]);
    assert_eq!(a, hashing::tree_digest([("a.php", "1"), ("b.php", "2")]));
    assert_ne!(a, hashing::tree_digest([("a.php", "1"), ("b.php", "3")]));
}

#[test]
fn release_notes_group_list_items_under_headings() {
    let notes = ReleaseNotes::from_html(
        "<h3>Added</h3>\n<ul><li>Update from the <b>toolbar</b>.</li><li>Settings window with tabs.</li></ul>\n<h3>Fixed</h3>\n<ul><li>No more <code>403</code> rate limit &amp; retries.</li></ul>",
    );
    assert_eq!(
        notes.sections,
        vec![
            Section {
                title: Some("Added".into()),
                items: vec![
                    "Update from the **toolbar**.".into(),
                    "Settings window with tabs.".into()
                ]
            },
            Section {
                title: Some("Fixed".into()),
                items: vec!["No more `403` rate limit & retries.".into()]
            },
        ]
    );
}

#[test]
fn release_notes_paragraph_plain_and_empty() {
    let paragraph = ReleaseNotes::from_html("<p>Update the SVN working copy; go Back.</p>");
    assert_eq!(
        paragraph.sections,
        vec![Section {
            title: None,
            items: vec!["Update the SVN working copy; go Back.".into()]
        }]
    );
    assert_eq!(
        ReleaseNotes::from_html("Faster reads.").sections[0].items,
        ["Faster reads."]
    );
    assert!(ReleaseNotes::from_html("  ").is_empty());
    assert!(ReleaseNotes::from_html("<h3>Added</h3>").is_empty());
    let link =
        ReleaseNotes::from_html("<ul><li>See the\n   <a href=\"https://example.com/log\">changelog</a>.</li></ul>");
    assert_eq!(
        link.sections[0].items,
        ["See the [changelog](https://example.com/log)."]
    );
}

#[test]
fn commit_templates() {
    assert_eq!(
        commit_template::render(
            "  Tagging {slug} {version} (was {previous}) ",
            "Sample",
            "sample-plugin",
            Some("1.1.0"),
            Some("1.0.0")
        ),
        "Tagging sample-plugin 1.1.0 (was 1.0.0)"
    );
    assert_eq!(
        commit_template::render("Release {version}", "S", "s", None, None),
        "Release"
    );
    assert_eq!(commit_template::problem("Release {name} {version}"), None);
    assert!(commit_template::problem("   ").is_some());
    assert!(commit_template::problem(&"x".repeat(301)).is_some());
    assert_eq!(
        commit_template::problem("Release {versoin}").as_deref(),
        Some("{versoin} is not a token. Use {name}, {slug}, {version}, {previous}.")
    );
}
