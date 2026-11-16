//! Crash files: what a panic leaves behind for `graft doctor --report`.
//!
//! [`install_panic_hook`] makes every panic in a binary also write a short
//! text file (version, message, location, backtrace) to
//! `$GRAFT_HOME/crashes/`. Only the newest [`KEEP`] files are kept. The
//! previous hook still runs, so the panic is printed as before.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::{GraftHome, Timestamp};

/// How many crash files are kept; older ones are removed.
pub const KEEP: usize = 20;

/// `$GRAFT_HOME/crashes`.
#[must_use]
pub fn crash_dir(home: &GraftHome) -> PathBuf {
    home.path().join("crashes")
}

/// Makes panics in this process also write a crash file for `binary`
/// (e.g. `graft`, `graftd`) under `home`. Writing it is best effort: a
/// failure is ignored, so the hook never panics itself.
pub fn install_panic_hook(home: &GraftHome, binary: &'static str, version: &'static str) {
    let dir = crash_dir(home);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(non-string panic payload)".to_owned());
        let location = info
            .location()
            .map_or_else(String::new, |l| format!("{}:{}", l.file(), l.line()));
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let _ = write_crash(&dir, binary, version, &message, &location, &backtrace);
        previous(info);
    }));
}

/// Writes one crash file and trims old ones; returns its path.
///
/// # Errors
///
/// If the directory or file cannot be written.
pub fn write_crash(
    dir: &Path,
    binary: &str,
    version: &str,
    message: &str,
    location: &str,
    backtrace: &str,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let now = Timestamp::now();
    let path = dir.join(format!(
        "{binary}-{}-{}.txt",
        now.as_millis(),
        std::process::id()
    ));
    let mut text = String::new();
    let _ = writeln!(text, "binary: {binary} {version}");
    let _ = writeln!(
        text,
        "platform: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let _ = writeln!(text, "time: {now}");
    let _ = writeln!(text, "pid: {}", std::process::id());
    let _ = writeln!(
        text,
        "thread: {}",
        std::thread::current().name().unwrap_or("?")
    );
    let _ = writeln!(text, "message: {message}");
    let _ = writeln!(text, "location: {location}");
    let _ = writeln!(text, "\n{backtrace}");
    std::fs::write(&path, text)?;
    trim(dir, KEEP);
    Ok(path)
}

/// Crash files in `dir`, newest first.
#[must_use]
pub fn crash_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "txt"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    files.into_iter().map(|(_, p)| p).collect()
}

fn trim(dir: &Path, keep: usize) {
    for old in crash_files(dir).into_iter().skip(keep) {
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_crash_file_with_the_panic_details() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_crash(dir.path(), "graft", "1.2.3", "boom", "src/x.rs:7", "bt").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        for want in [
            "binary: graft 1.2.3",
            "message: boom",
            "location: src/x.rs:7",
            "bt",
        ] {
            assert!(text.contains(want), "{want:?} missing from:\n{text}");
        }
        assert_eq!(crash_files(dir.path()), vec![path]);
    }

    #[test]
    fn keeps_only_the_newest_files() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..KEEP + 5 {
            std::fs::write(dir.path().join(format!("graft-{i:03}-1.txt")), "x").unwrap();
        }
        trim(dir.path(), KEEP);
        assert_eq!(crash_files(dir.path()).len(), KEEP);
    }

    #[test]
    fn a_missing_directory_has_no_crash_files() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(crash_files(&dir.path().join("nope")), Vec::<PathBuf>::new());
    }
}
