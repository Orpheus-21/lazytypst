//! A local history of the saved text of each file. It lives in the state folder, never in the project, so
//! the project stays clean. Each version is a full copy in its own file. The store keeps at most
//! `MAX_VERSIONS` for each file and `MAX_TOTAL` bytes for all files, and it drops the oldest first.

use std::{
    fs, io,
    os::unix::{ffi::OsStrExt, fs::DirBuilderExt},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// The most versions of one file.
pub const MAX_VERSIONS: usize = 50;
/// The most bytes of all versions of all files.
pub const MAX_TOTAL: u64 = 20 << 20;
/// A save makes a new version only if the newest one is at least this old.
pub const MIN_GAP: Duration = Duration::from_secs(30);
/// A text larger than this has no history. It is almost never a Typst file.
pub const MAX_TEXT: usize = 2 << 20;

/// One saved version of a file.
#[derive(Clone, Debug)]
pub struct Version {
    pub time: SystemTime,
    pub size: u64,
    path: PathBuf,
}

impl Version {
    pub fn read(&self) -> io::Result<String> {
        crate::fsutil::read_text(&self.path)
    }
}

/// The 64 bit FNV-1a hash of the bytes. It names the folder of a file. The hash of the standard library
/// is not fixed between versions of Rust, and the names must stay the same.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn folder_of(history: &Path, file: &Path) -> PathBuf {
    history.join(format!("{:016x}", fnv1a(file.as_os_str().as_bytes())))
}

fn millis(time: SystemTime) -> u128 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_millis())
}

/// The versions in `folder`, the newest first. A file with another name is not a version.
fn versions_in(folder: &Path) -> Vec<Version> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut versions: Vec<Version> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let ms: u64 = name.strip_suffix(".txt")?.parse().ok()?;
            let meta = entry.metadata().ok().filter(fs::Metadata::is_file)?;
            Some(Version {
                time: UNIX_EPOCH + Duration::from_millis(ms),
                size: meta.len(),
                path: entry.path(),
            })
        })
        .collect();
    versions.sort_by_key(|version| std::cmp::Reverse(version.time));
    versions
}

/// The versions of `file`, the newest first.
pub fn list(history: &Path, file: &Path) -> Vec<Version> {
    versions_in(&folder_of(history, file))
}

/// Keeps `text` as a new version of `file` at `now`. It does nothing, and returns false, if the text is
/// the same as the newest version, if the newest version is younger than `MIN_GAP` (unless `force`), or if
/// the text is larger than `MAX_TEXT`. Then it drops the oldest versions over the limits.
pub fn record(
    history: &Path,
    file: &Path,
    text: &str,
    now: SystemTime,
    force: bool,
) -> io::Result<bool> {
    if text.len() > MAX_TEXT {
        return Ok(false);
    }
    let folder = folder_of(history, file);
    if let Some(newest) = versions_in(&folder).first() {
        let young = now
            .duration_since(newest.time)
            .is_ok_and(|age| age < MIN_GAP);
        if (young && !force) || newest.read().is_ok_and(|old| old == text) {
            return Ok(false);
        }
    }
    // The folders are for the user only: they hold copies of the documents.
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&folder)?;
    let path = folder.join(format!("{:020}.txt", millis(now)));
    crate::fsutil::write_file(&path, text.as_bytes())?;
    prune(history, &folder, &path);
    Ok(true)
}

/// Drops the oldest versions of the folder over `MAX_VERSIONS`, and then the oldest versions of all files
/// over `MAX_TOTAL`. The version `keep` always stays.
fn prune(history: &Path, folder: &Path, keep: &Path) {
    for version in versions_in(folder).into_iter().skip(MAX_VERSIONS) {
        let _ = fs::remove_file(version.path);
    }
    let mut all: Vec<Version> = fs::read_dir(history)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|entry| versions_in(&entry.path()))
        .collect();
    let mut total: u64 = all.iter().map(|version| version.size).sum();
    all.sort_by_key(|version| version.time);
    for version in all {
        if total <= MAX_TOTAL {
            break;
        }
        if version.path != keep && fs::remove_file(&version.path).is_ok() {
            total -= version.size;
        }
    }
}

/// How many lines of `old` are not in `new`, and how many lines of `new` are not in `old`. A line that
/// appears twice counts twice. It is a quick summary of a change, not a diff.
pub fn changed_lines(old: &str, new: &str) -> (usize, usize) {
    use std::collections::HashMap;
    let mut counts: HashMap<&str, isize> = HashMap::new();
    for line in old.lines() {
        *counts.entry(line).or_default() += 1;
    }
    for line in new.lines() {
        *counts.entry(line).or_default() -= 1;
    }
    let removed = counts.values().filter(|count| **count > 0).sum::<isize>();
    let added = -counts.values().filter(|count| **count < 0).sum::<isize>();
    (removed.unsigned_abs(), added.unsigned_abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lazytypst-history-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_000_000 + seconds)
    }

    #[test]
    fn a_save_makes_a_version_only_if_the_text_is_new_and_the_newest_is_old_enough() {
        let dir = temp("gap");
        let file = Path::new("/p/doc.typ");
        assert!(record(&dir, file, "one", at(0), false).unwrap());
        assert!(
            !record(&dir, file, "one", at(100), false).unwrap(),
            "the same text"
        );
        assert!(
            !record(&dir, file, "two", at(10), false).unwrap(),
            "too soon"
        );
        assert!(record(&dir, file, "two", at(10), true).unwrap(), "forced");
        assert!(record(&dir, file, "three", at(60), false).unwrap());
        let versions = list(&dir, file);
        let texts: Vec<_> = versions.iter().map(|v| v.read().unwrap()).collect();
        assert_eq!(texts, ["three", "two", "one"], "the newest first");
        assert_eq!(versions[0].size, 5);
        assert!(
            list(&dir, Path::new("/p/other.typ")).is_empty(),
            "another file has its own history"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_keeps_at_most_fifty_versions_and_the_oldest_go_first() {
        let dir = temp("fifty");
        let file = Path::new("/p/doc.typ");
        for n in 0..60u64 {
            assert!(record(&dir, file, &format!("text {n}"), at(n * 100), false).unwrap());
        }
        let versions = list(&dir, file);
        assert_eq!(versions.len(), MAX_VERSIONS);
        assert_eq!(versions[0].read().unwrap(), "text 59");
        assert_eq!(versions.last().unwrap().read().unwrap(), "text 10");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn all_files_together_keep_at_most_the_total_and_the_new_version_stays() {
        let dir = temp("total");
        let big = "x".repeat(MAX_TEXT - 10);
        // 11 versions of 2 MiB are 22 MiB, over the limit of 20 MiB.
        for n in 0..11u64 {
            let file = PathBuf::from(format!("/p/doc{n}.typ"));
            record(&dir, &file, &format!("{big}{n}"), at(n), true).unwrap();
        }
        let total: u64 = (0..11)
            .map(|n| {
                list(&dir, Path::new(&format!("/p/doc{n}.typ")))
                    .iter()
                    .map(|v| v.size)
                    .sum::<u64>()
            })
            .sum();
        assert!(total <= MAX_TOTAL, "{total}");
        assert_eq!(
            list(&dir, Path::new("/p/doc10.typ")).len(),
            1,
            "the newest stays"
        );
        assert!(
            list(&dir, Path::new("/p/doc0.typ")).is_empty(),
            "the oldest went"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_text_over_the_limit_has_no_history_and_the_folder_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp("private");
        let file = Path::new("/p/doc.typ");
        assert!(!record(&dir, file, &"x".repeat(MAX_TEXT + 1), at(0), true).unwrap());
        assert!(list(&dir, file).is_empty());
        record(&dir, file, "small", at(0), true).unwrap();
        let mode = fs::metadata(folder_of(&dir, file))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "{mode:o}");
        fs::write(folder_of(&dir, file).join("notes.txt"), "no").unwrap();
        assert_eq!(list(&dir, file).len(), 1, "a foreign file is not a version");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_summary_counts_lines_that_left_and_lines_that_came() {
        assert_eq!(changed_lines("a\nb\nc\n", "a\nb\nc\n"), (0, 0));
        assert_eq!(changed_lines("a\nb\nc\n", "a\nx\nc\ny\n"), (1, 2));
        assert_eq!(
            changed_lines("a\na\n", "a\n"),
            (1, 0),
            "a repeated line counts twice"
        );
        assert_eq!(changed_lines("", "a\n"), (0, 1));
    }
}
