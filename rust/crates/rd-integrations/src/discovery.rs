//! Where `git`, `php` and `wp` are looked for. The Swift app only checks fixed macOS
//! locations; here every platform has its own list, and the executable on the parent
//! process's `PATH` is tried last (as `rd-svn` does for `svn`).

use rd_svn::process_runner::is_executable_file;
use std::path::PathBuf;

/// Git: the Swift list on macOS (Xcode Command Line Tools, Homebrew on Apple silicon and
/// Intel), distribution packages on Linux, Git for Windows on Windows.
#[cfg(target_os = "macos")]
pub const GIT_CANDIDATES: &[&str] = &["/usr/bin/git", "/opt/homebrew/bin/git", "/usr/local/bin/git"];
#[cfg(all(unix, not(target_os = "macos")))]
pub const GIT_CANDIDATES: &[&str] = &[
    "/usr/bin/git",
    "/usr/local/bin/git",
    "/home/linuxbrew/.linuxbrew/bin/git",
];
#[cfg(windows)]
pub const GIT_CANDIDATES: &[&str] = &[
    r"C:\Program Files\Git\cmd\git.exe",
    r"C:\Program Files\Git\bin\git.exe",
    r"C:\Program Files (x86)\Git\cmd\git.exe",
];

/// PHP: the Swift list on macOS; `/usr/bin/php` (php-cli packages) first on Linux; the
/// usual manual and Chocolatey install folders on Windows (Scoop and winget put `php.exe`
/// on `PATH`).
#[cfg(target_os = "macos")]
pub const PHP_CANDIDATES: &[&str] = &["/opt/homebrew/bin/php", "/usr/local/bin/php", "/usr/bin/php"];
#[cfg(all(unix, not(target_os = "macos")))]
pub const PHP_CANDIDATES: &[&str] = &[
    "/usr/bin/php",
    "/usr/local/bin/php",
    "/home/linuxbrew/.linuxbrew/bin/php",
];
#[cfg(windows)]
pub const PHP_CANDIDATES: &[&str] = &[
    r"C:\php\php.exe",
    r"C:\Program Files\PHP\php.exe",
    r"C:\tools\php\php.exe",
];

/// WP-CLI: the Swift list on macOS; `/usr/local/bin/wp` (the documented install location)
/// on Linux. On Windows WP-CLI is a `wp.bat` wrapper somewhere on `PATH`.
#[cfg(target_os = "macos")]
pub const WP_CANDIDATES: &[&str] = &["/opt/homebrew/bin/wp", "/usr/local/bin/wp"];
#[cfg(all(unix, not(target_os = "macos")))]
pub const WP_CANDIDATES: &[&str] = &["/usr/local/bin/wp", "/usr/bin/wp", "/home/linuxbrew/.linuxbrew/bin/wp"];
#[cfg(windows)]
pub const WP_CANDIDATES: &[&str] = &[];

/// The first executable in `candidates`, else `name` on `PATH` (`which`, which honours
/// `PATHEXT` on Windows, so `wp` finds `wp.bat`).
pub fn find_executable(candidates: &[&str], name: &str) -> Option<PathBuf> {
    candidates
        .iter()
        .map(PathBuf::from)
        .find(|path| is_executable_file(path))
        .or_else(|| which::which(name).ok().filter(|path| is_executable_file(path)))
}
