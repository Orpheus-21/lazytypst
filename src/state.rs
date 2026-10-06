//! The main file that the user chose for each project. It is stored outside the project folder.

use std::{
    ffi::{OsStr, OsString},
    fs, io,
    os::unix::ffi::OsStrExt,
    path::{Component, Path, PathBuf},
};

/// The file that holds the main files. Each line is `<project folder>`, a tab, and `<main file>`.
/// The main file is a path relative to the project folder. Paths are bytes, so any name works.
/// The location follows the XDG rules: `$XDG_STATE_HOME`, or else `$HOME/.local/state`.
/// A relative value is ignored, as the XDG rules say.
pub fn state_file(xdg_state_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let absolute = |value: Option<OsString>| value.map(PathBuf::from).filter(|path| path.is_absolute());
    let base = absolute(xdg_state_home).or_else(|| absolute(home).map(|home| home.join(".local").join("state")))?;
    Some(base.join("lazytypst").join("main-files"))
}

/// The state file of the user, or `None` if the environment names no usable place.
pub fn default_state_file() -> Option<PathBuf> {
    state_file(std::env::var_os("XDG_STATE_HOME"), std::env::var_os("HOME"))
}

/// The saved main file of the project `root`, if the file still exists inside the project.
/// A missing file, a broken file, and a saved path that leaves the project all give `None`.
pub fn load_main(file: &Path, root: &Path) -> Option<PathBuf> {
    let bytes = fs::read(file).ok()?;
    let main = bytes
        .split(|byte| *byte == b'\n')
        .filter_map(|line| split_line(line).filter(|(saved_root, _)| *saved_root == root.as_os_str().as_bytes()))
        .map(|(_, main)| PathBuf::from(OsStr::from_bytes(main)))
        .next_back()?;
    // The file is edited by hand sometimes. Trust only a plain relative path.
    let plain = main.components().all(|part| matches!(part, Component::Normal(_)));
    (plain && root.join(&main).is_file()).then_some(main)
}

/// Saves the main file of the project `root`, or removes the choice if `main` is `None`.
/// The lines of other projects stay. A path with a tab or a line break cannot be saved, because
/// the format uses them as separators.
pub fn save_main(file: &Path, root: &Path, main: Option<&Path>) -> io::Result<()> {
    let root_bytes = root.as_os_str().as_bytes();
    let main_bytes = main.map(|main| main.as_os_str().as_bytes());
    let bad = |bytes: &[u8]| bytes.iter().any(|byte| matches!(byte, b'\t' | b'\n'));
    if bad(root_bytes) || main_bytes.is_some_and(bad) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a path with a tab or a line break cannot be saved",
        ));
    }
    let old = match fs::read(file) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(err) => return Err(err),
    };
    let mut new = Vec::new();
    for line in old.split(|byte| *byte == b'\n').filter(|line| !line.is_empty()) {
        if split_line(line).is_none_or(|(saved_root, _)| saved_root != root_bytes) {
            new.extend_from_slice(line);
            new.push(b'\n');
        }
    }
    if let Some(main) = main_bytes {
        new.extend_from_slice(root_bytes);
        new.push(b'\t');
        new.extend_from_slice(main);
        new.push(b'\n');
    }
    if new.is_empty() && old.is_empty() {
        return Ok(()); // nothing to remove, and no file to make
    }
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    crate::fsutil::write_file(file, &new)
}

/// Splits a line at its first tab. A line with no tab, or with an empty part, is not valid.
fn split_line(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let tab = line.iter().position(|byte| *byte == b'\t')?;
    let (root, main) = (&line[..tab], &line[tab + 1..]);
    (!root.is_empty() && !main.is_empty()).then_some((root, main))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lazytypst-state-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Makes a project folder with the files `main.typ` and `chapters/one.typ`.
    fn project(dir: &Path, name: &str) -> PathBuf {
        let root = dir.join(name);
        fs::create_dir_all(root.join("chapters")).unwrap();
        fs::write(root.join("main.typ"), "").unwrap();
        fs::write(root.join("chapters").join("one.typ"), "").unwrap();
        root
    }

    fn os(text: &str) -> Option<OsString> {
        Some(OsString::from(text))
    }

    #[test]
    fn an_absolute_xdg_state_home_decides_the_file() {
        assert_eq!(
            state_file(os("/x/state"), os("/home/u")),
            Some(PathBuf::from("/x/state/lazytypst/main-files"))
        );
    }

    #[test]
    fn a_relative_or_empty_xdg_state_home_is_ignored() {
        let expected = Some(PathBuf::from("/home/u/.local/state/lazytypst/main-files"));
        assert_eq!(state_file(os("relative/dir"), os("/home/u")), expected);
        assert_eq!(state_file(os(""), os("/home/u")), expected);
        assert_eq!(state_file(None, os("/home/u")), expected);
    }

    #[test]
    fn without_a_usable_home_there_is_no_state_file() {
        assert_eq!(state_file(None, None), None);
        assert_eq!(state_file(None, os("")), None);
        assert_eq!(state_file(None, os("relative")), None);
    }

    #[test]
    fn a_saved_main_file_loads_again() {
        let dir = temp_dir("roundtrip");
        let root = project(&dir, "book");
        let file = dir.join("state").join("main-files");
        save_main(&file, &root, Some(Path::new("main.typ"))).unwrap();
        assert_eq!(load_main(&file, &root), Some(PathBuf::from("main.typ")));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn each_project_keeps_its_own_main_file() {
        let dir = temp_dir("projects");
        let (one, two) = (project(&dir, "one"), project(&dir, "two"));
        let file = dir.join("main-files");
        save_main(&file, &one, Some(Path::new("main.typ"))).unwrap();
        save_main(&file, &two, Some(Path::new("chapters/one.typ"))).unwrap();
        assert_eq!(load_main(&file, &one), Some(PathBuf::from("main.typ")));
        assert_eq!(load_main(&file, &two), Some(PathBuf::from("chapters/one.typ")));

        // A new choice replaces the old choice of the same project only.
        save_main(&file, &one, Some(Path::new("chapters/one.typ"))).unwrap();
        assert_eq!(load_main(&file, &one), Some(PathBuf::from("chapters/one.typ")));
        assert_eq!(load_main(&file, &two), Some(PathBuf::from("chapters/one.typ")));
        assert_eq!(fs::read_to_string(&file).unwrap().lines().count(), 2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn removing_the_mark_removes_only_that_project() {
        let dir = temp_dir("remove");
        let (one, two) = (project(&dir, "one"), project(&dir, "two"));
        let file = dir.join("main-files");
        save_main(&file, &one, Some(Path::new("main.typ"))).unwrap();
        save_main(&file, &two, Some(Path::new("main.typ"))).unwrap();
        save_main(&file, &one, None).unwrap();
        assert_eq!(load_main(&file, &one), None);
        assert_eq!(load_main(&file, &two), Some(PathBuf::from("main.typ")));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn removing_a_mark_that_was_never_saved_creates_no_file() {
        let dir = temp_dir("noop");
        let root = project(&dir, "book");
        let file = dir.join("never").join("main-files");
        save_main(&file, &root, None).unwrap();
        assert!(!file.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_saved_main_file_that_is_gone_is_ignored() {
        let dir = temp_dir("gone");
        let root = project(&dir, "book");
        let file = dir.join("main-files");
        save_main(&file, &root, Some(Path::new("main.typ"))).unwrap();
        fs::remove_file(root.join("main.typ")).unwrap();
        assert_eq!(load_main(&file, &root), None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_or_broken_state_file_gives_no_main_file() {
        let dir = temp_dir("broken");
        let root = project(&dir, "book");
        assert_eq!(load_main(&dir.join("missing"), &root), None);

        let file = dir.join("main-files");
        fs::write(&file, b"\xff\xfe\n\t\n no tab here\n\n\t\t\t\n").unwrap();
        assert_eq!(load_main(&file, &root), None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_main_path_that_leaves_the_project_is_ignored() {
        let dir = temp_dir("escape");
        let root = project(&dir, "book");
        fs::write(dir.join("outside.typ"), "").unwrap();
        let file = dir.join("main-files");
        for bad in ["../outside.typ", "/etc/passwd", "chapters/../../outside.typ"] {
            fs::write(&file, format!("{}\t{bad}\n", root.display())).unwrap();
            assert_eq!(load_main(&file, &root), None, "{bad}");
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_path_with_a_tab_or_a_line_break_is_refused_and_the_file_stays() {
        let dir = temp_dir("refuse");
        let root = project(&dir, "book");
        let file = dir.join("main-files");
        save_main(&file, &root, Some(Path::new("main.typ"))).unwrap();
        let before = fs::read(&file).unwrap();

        for bad in ["a\tb.typ", "a\nb.typ"] {
            let err = save_main(&file, &root, Some(Path::new(bad))).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        }
        let odd_root = dir.join("odd\tfolder");
        assert!(save_main(&file, &odd_root, Some(Path::new("main.typ"))).is_err());
        assert_eq!(fs::read(&file).unwrap(), before);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_folder_name_that_is_not_utf8_works() {
        let dir = temp_dir("bytes");
        let root = dir.join(OsString::from_vec(b"dir-\xff".to_vec()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("main.typ"), "").unwrap();
        let file = dir.join("main-files");
        save_main(&file, &root, Some(Path::new("main.typ"))).unwrap();
        assert_eq!(load_main(&file, &root), Some(PathBuf::from("main.typ")));
        fs::remove_dir_all(dir).unwrap();
    }
}
