//! Safe reads and writes of files.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// The most bytes that the editor opens. A larger file is almost never a Typst file.
pub const MAX_TEXT_BYTES: u64 = 16 << 20;

/// Reads the text of the file at `path`. The path must lead to a regular file of at most `MAX_TEXT_BYTES`.
/// A pipe or a device would never end, and the read would hold the program with the terminal in raw mode.
pub fn read_text(path: &Path) -> io::Result<String> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "it is not a regular file",
        ));
    }
    if meta.len() > MAX_TEXT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("the file is larger than {} MiB", MAX_TEXT_BYTES >> 20),
        ));
    }
    fs::read_to_string(path)
}

/// Writes `bytes` to the file at `path` so that a crash leaves the old file or the new file, never a cut file.
/// The text goes to a new hidden temp file next to the real file. Then a rename replaces the real file.
/// A symlink at `path` stays a symlink, and the permissions stay. A hard link to the old file keeps the old text.
pub fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let target = match fs::canonicalize(path) {
        Ok(target) => target,
        // The file is gone. `create_new` also refuses a dangling symlink at `path`.
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?;
            return file.write_all(bytes).and_then(|()| file.sync_all());
        }
        Err(err) => return Err(err),
    };
    // A rename can replace a file that the user cannot write. This open makes the same check as a direct write.
    fs::OpenOptions::new().write(true).open(&target)?;
    let (mut file, temp) = create_temp_beside(&target)?;
    let result = file
        .write_all(bytes)
        .and_then(|()| file.set_permissions(fs::metadata(&target)?.permissions()))
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::rename(&temp, &target));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Creates a new hidden temp file next to `target` and returns it with its path.
/// `create_new` refuses every existing path, also a symlink, so a link that someone planted
/// can never redirect the write. A name that is taken is skipped.
fn create_temp_beside(target: &Path) -> io::Result<(fs::File, PathBuf)> {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |time| time.subsec_nanos());
    for attempt in 0..100 {
        let temp = target.with_file_name(format!(
            ".{name}.{}-{stamp}-{attempt}.lazytypst-tmp",
            std::process::id()
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
        {
            Ok(file) => return Ok((file, temp)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free name for the temp file",
    ))
}

/// How old a leftover temp file must be before the program deletes it.
pub const LEFTOVER_AGE: Duration = Duration::from_secs(3600);

/// The process id in the name of a temp file that this program makes, or `None` for any other name.
/// The names are `.<name>.<pid>-<stamp>-<attempt>.lazytypst-tmp` (a save) and `.lazytypst-<pid>-<n>.pdf.tmp`
/// (an export).
fn leftover_pid(name: &str) -> Option<u32> {
    let rest = if let Some(inner) = name.strip_suffix(".lazytypst-tmp") {
        let (_, tail) = inner.rsplit_once('.')?;
        let mut parts = tail.split('-');
        let pid = parts.next()?;
        // The stamp and the attempt follow. Both are numbers.
        let numbers = parts.clone().count() == 2 && parts.all(|part| part.parse::<u32>().is_ok());
        numbers.then_some(pid)?
    } else {
        let inner = name.strip_prefix(".lazytypst-")?.strip_suffix(".pdf.tmp")?;
        let (pid, count) = inner.split_once('-')?;
        count.parse::<u32>().ok()?;
        pid
    };
    rest.parse().ok()
}

/// Deletes the temp files that an earlier run of this program left in `folders`: a regular file (not a
/// link) whose name matches, older than `LEFTOVER_AGE`, and whose process `alive` says is gone. A kill
/// between the temp file and the rename leaves such a file. It returns how many files it deleted.
pub fn remove_leftovers(
    folders: impl IntoIterator<Item = PathBuf>,
    now: SystemTime,
    alive: impl Fn(u32) -> bool,
) -> usize {
    let mut removed = 0;
    for folder in folders {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name.to_str().and_then(leftover_pid) else {
                continue;
            };
            let Ok(meta) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            let old = meta
                .modified()
                .ok()
                .and_then(|time| now.duration_since(time).ok())
                .is_some_and(|age| age >= LEFTOVER_AGE);
            if meta.is_file() && old && !alive(pid) && fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lazytypst-fsutil-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn bytes_that_are_not_utf8_are_written_unchanged() {
        let dir = temp_dir("bytes");
        let path = dir.join("state");
        let bytes = b"a\xff\xfeb\n";
        write_file(&path, bytes).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        write_file(&path, b"new\n").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new\n");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_planted_symlink_at_a_temp_name_is_not_followed() {
        let dir = temp_dir("planted");
        let path = dir.join("state");
        fs::write(&path, "old").unwrap();
        let victim = dir.join("victim");
        fs::write(&victim, "keep").unwrap();
        // The temp name contains the pid, so plant links for the first attempts that the function can make.
        for attempt in 0..3 {
            let name = format!(".state.{}-0-{attempt}.lazytypst-tmp", std::process::id());
            let _ = std::os::unix::fs::symlink(&victim, dir.join(name));
        }
        write_file(&path, b"new").unwrap();
        assert_eq!(fs::read(&victim).unwrap(), b"keep");
        assert_eq!(fs::read(&path).unwrap(), b"new");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn read_text_refuses_a_pipe_a_device_and_a_big_file() {
        let dir = std::env::temp_dir().join(format!("lazytypst-readtext-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("ok.typ"), "text").unwrap();
        assert_eq!(read_text(&dir.join("ok.typ")).unwrap(), "text");
        assert!(read_text(Path::new("/dev/zero")).is_err());
        let fifo = dir.join("pipe.typ");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        assert!(read_text(&fifo).is_err());
        let big = fs::File::create(dir.join("big.typ")).unwrap();
        big.set_len(MAX_TEXT_BYTES + 1).unwrap();
        let err = read_text(&dir.join("big.typ")).unwrap_err();
        assert!(err.to_string().contains("larger than 16 MiB"), "{err}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn only_leftovers_of_our_own_names_with_a_dead_process_and_old_age_go() {
        let dir = temp_dir("leftovers");
        let make = |name: &str| fs::write(dir.join(name), "x").unwrap();
        let old = [
            ".main.typ.4000000-123-0.lazytypst-tmp",
            ".lazytypst-4000000-3.pdf.tmp",
        ];
        let kept = [
            ".main.typ.4000001-123-0.lazytypst-tmp", // the process runs
            ".notes.txt",
            "main.typ",
            ".main.typ.x-123-0.lazytypst-tmp",     // not a pid
            ".main.typ.4000000-123.lazytypst-tmp", // no attempt
            ".lazytypst-4000000-3.pdf",
            "lazytypst-4000000-3.pdf.tmp", // not hidden
            ".other.4000000-1-0.tmp",
        ];
        for name in old.iter().chain(&kept) {
            make(name);
        }
        let now = SystemTime::now() + LEFTOVER_AGE + Duration::from_secs(1);
        let alive = |pid: u32| pid == 4000001;
        // A young file stays. The files above are as old as `now - LEFTOVER_AGE`, so use the clock as is.
        assert_eq!(remove_leftovers([dir.clone()], SystemTime::now(), alive), 0);
        assert_eq!(remove_leftovers([dir.clone()], now, alive), old.len());
        for name in old {
            assert!(!dir.join(name).exists(), "{name}");
        }
        for name in kept {
            assert!(dir.join(name).exists(), "{name}");
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_leftover_that_is_a_link_or_a_folder_stays() {
        let dir = temp_dir("leftoverlink");
        let target = dir.join("real.txt");
        fs::write(&target, "x").unwrap();
        std::os::unix::fs::symlink(&target, dir.join(".a.4000000-1-0.lazytypst-tmp")).unwrap();
        fs::create_dir(dir.join(".lazytypst-4000000-1.pdf.tmp")).unwrap();
        let now = SystemTime::now() + LEFTOVER_AGE + Duration::from_secs(1);
        assert_eq!(remove_leftovers([dir.clone()], now, |_| false), 0);
        assert!(target.exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
