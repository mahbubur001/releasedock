//! Ported from CoreTests.swift (KeychainTests).

use rd_keychain::*;

#[test]
fn in_memory_keychain() {
    let keychain = InMemoryKeychain::new();
    let item = KeychainItem::svn("plugins.svn.wordpress.org", "alice");
    assert_eq!(item.service, "com.releasedock.svn.plugins.svn.wordpress.org");
    assert_eq!(keychain.password(&item).unwrap(), None);
    keychain.set_password("s3cret", &item).unwrap();
    assert_eq!(keychain.password(&item).unwrap().as_deref(), Some("s3cret"));
    keychain.delete_password(&item).unwrap();
    assert_eq!(keychain.password(&item).unwrap(), None);
}

/// Touches the real credential store (macOS may ask for permission; Linux needs a running
/// Secret Service). Run with `cargo test -p rd-keychain -- --ignored`.
#[test]
#[ignore]
fn real_keychain_round_trip() {
    let keychain = NativeKeychain;
    let item = KeychainItem::new(format!("com.releasedock.tests.{}", std::process::id()), "tester");
    keychain.set_password("first", &item).unwrap();
    keychain.set_password("second", &item).unwrap();
    assert_eq!(keychain.password(&item).unwrap().as_deref(), Some("second"));
    keychain.delete_password(&item).unwrap();
    assert_eq!(keychain.password(&item).unwrap(), None);
    keychain.delete_password(&item).unwrap();
}
