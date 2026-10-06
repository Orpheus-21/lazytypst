//! Safe writes to files.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

/// Writes `bytes` to the file at `path` so that a crash leaves the old file or the new file, never a cut file.
/// The text goes to a new hidden temp file next to the real file. Then a rename replaces the real file.
/// A symlink at `path` stays a symlink, and the permissions stay. A hard link to the old file keeps the old text.
pub fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let target = match fs::canonicalize(path) {
        Ok(target) => target,
        // The file is gone. `create_new` also refuses a dangling symlink at `path`.
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path)?;
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
        match fs::OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => return Ok((file, temp)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "no free name for the temp file"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lazytypst-fsutil-{name}-{}", std::process::id()));
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
}
