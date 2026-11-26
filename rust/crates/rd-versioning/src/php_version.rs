//! A faithful port of PHP's `version_compare()` (php-src `ext/standard/versioning.c`).
//!
//! WordPress core and WordPress.org compare plugin and theme versions with
//! `version_compare()`, so ReleaseDock uses identical semantics rather than SemVer:
//!
//! - `-`, `_`, `+` and other non-alphanumerics become `.`
//! - a `.` is inserted between digit/non-digit transitions (`1.0rc1` → `1.0.rc.1`)
//! - special forms order: any unknown string < `dev` < `alpha`=`a` < `beta`=`b`
//!   < `RC`=`rc` < number < `pl`=`p`

use std::borrow::Cow;
use std::cmp::Ordering;

/// Returns -1, 0 or 1 exactly like `version_compare($a, $b)`.
pub fn compare(a: &str, b: &str) -> i32 {
    version_compare(a.as_bytes(), b.as_bytes())
}

/// [`compare`] as an [`Ordering`].
pub fn ordering(a: &str, b: &str) -> Ordering {
    compare(a, b).cmp(&0)
}

/// The canonical form PHP compares (`php_canonicalize_version`).
pub fn canonicalize(version: &str) -> String {
    String::from_utf8_lossy(&canonicalize_bytes(version.as_bytes())).into_owned()
}

// MARK: - Port

const DOT: u8 = b'.';
const NUMBER_MARKER: &[u8] = b"#N#";

fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

fn is_alnum(c: u8) -> bool {
    c.is_ascii_alphanumeric()
}

fn isdig(c: u8) -> bool {
    is_digit(c) && c != DOT
}

fn isndig(c: u8) -> bool {
    !is_digit(c) && c != DOT
}

fn is_special_ver(c: u8) -> bool {
    c == b'-' || c == b'_' || c == b'+'
}

pub(crate) fn canonicalize_bytes(version: &[u8]) -> Vec<u8> {
    let Some((&first, rest)) = version.split_first() else {
        return Vec::new();
    };
    let mut q = vec![first];
    let mut lp = first;
    for &p in rest {
        let lq = q.last().copied().unwrap_or(first);
        if is_special_ver(p) {
            if lq != DOT {
                q.push(DOT);
            }
        } else if (isndig(lp) && isdig(p)) || (isdig(lp) && isndig(p)) {
            if lq != DOT {
                q.push(DOT);
            }
            q.push(p);
        } else if !is_alnum(p) {
            if lq != DOT {
                q.push(DOT);
            }
        } else {
            q.push(p);
        }
        lp = p;
    }
    if q.last() == Some(&DOT) {
        q.pop();
    }
    q
}

const SPECIAL_FORMS: [(&[u8], i32); 10] = [
    (b"dev", 0),
    (b"alpha", 1),
    (b"a", 1),
    (b"beta", 2),
    (b"b", 2),
    (b"RC", 3),
    (b"rc", 3),
    (b"#", 4),
    (b"pl", 5),
    (b"p", 5),
];

/// `strncmp(form, name, strlen(name)) == 0` — i.e. `form` starts with `name`.
fn special_order(form: &[u8]) -> i32 {
    SPECIAL_FORMS
        .iter()
        .find(|(name, _)| form.starts_with(name))
        .map_or(-1, |&(_, order)| order)
}

fn normalize(value: i32) -> i32 {
    value.signum()
}

fn compare_special(a: &[u8], b: &[u8]) -> i32 {
    normalize(special_order(a) - special_order(b))
}

fn compare_numeric(a: &[u8], b: &[u8]) -> i32 {
    let digits = |s: &[u8]| -> Vec<u8> {
        s.iter()
            .copied()
            .skip_while(|&c| c == b'0')
            .take_while(|&c| is_digit(c))
            .collect()
    };
    let (n1, n2) = (digits(a), digits(b));
    if n1.len() != n2.len() {
        return if n1.len() < n2.len() { -1 } else { 1 };
    }
    match n1.cmp(&n2) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

fn find_dot(v: &[u8], from: usize) -> Option<usize> {
    v[from..].iter().position(|&c| c == DOT).map(|i| i + from)
}

pub(crate) fn version_compare(orig1: &[u8], orig2: &[u8]) -> i32 {
    if orig1.is_empty() || orig2.is_empty() {
        if orig1.is_empty() && orig2.is_empty() {
            return 0;
        }
        return if orig1.is_empty() { -1 } else { 1 };
    }
    let v1: Cow<[u8]> = if orig1[0] == b'#' {
        Cow::Borrowed(orig1)
    } else {
        Cow::Owned(canonicalize_bytes(orig1))
    };
    let v2: Cow<[u8]> = if orig2[0] == b'#' {
        Cow::Borrowed(orig2)
    } else {
        Cow::Owned(canonicalize_bytes(orig2))
    };

    // Emulates the C loop: p = start of current component, n = index of the next '.' (or None).
    let (mut p1, mut p2) = (0usize, 0usize);
    let (mut n1, mut n2): (Option<usize>, Option<usize>) = (Some(0), Some(0));
    let mut compare = 0;

    while p1 < v1.len() && p2 < v2.len() && n1.is_some() && n2.is_some() {
        n1 = find_dot(&v1, p1);
        n2 = find_dot(&v2, p2);
        let c1 = &v1[p1..n1.unwrap_or(v1.len())];
        let c2 = &v2[p2..n2.unwrap_or(v2.len())];
        let d1 = c1.first().is_some_and(|&c| is_digit(c));
        let d2 = c2.first().is_some_and(|&c| is_digit(c));
        compare = if d1 && d2 {
            compare_numeric(c1, c2)
        } else if !d1 && !d2 {
            compare_special(c1, c2)
        } else if d1 {
            compare_special(NUMBER_MARKER, c2)
        } else {
            compare_special(c1, NUMBER_MARKER)
        };
        if compare != 0 {
            break;
        }
        if let Some(n) = n1 {
            p1 = n + 1;
        }
        if let Some(n) = n2 {
            p2 = n + 1;
        }
    }

    if compare == 0 {
        if n1.is_some() {
            let rest = &v1[p1.min(v1.len())..];
            compare = if rest.first().is_some_and(|&c| is_digit(c)) {
                1
            } else {
                version_compare(rest, NUMBER_MARKER)
            };
        } else if n2.is_some() {
            let rest = &v2[p2.min(v2.len())..];
            compare = if rest.first().is_some_and(|&c| is_digit(c)) {
                -1
            } else {
                version_compare(NUMBER_MARKER, rest)
            };
        }
    }
    compare
}
