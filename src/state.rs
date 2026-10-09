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
    let absolute =
        |value: Option<OsString>| value.map(PathBuf::from).filter(|path| path.is_absolute());
    let base = absolute(xdg_state_home)
        .or_else(|| absolute(home).map(|home| home.join(".local").join("state")))?;
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
        .filter_map(|line| {
            split_line(line).filter(|(saved_root, _)| *saved_root == root.as_os_str().as_bytes())
        })
        .map(|(_, main)| PathBuf::from(OsStr::from_bytes(main)))
        .next_back()?;
    // The file is edited by hand sometimes. Trust only a plain relative path.
    let plain = main
        .components()
        .all(|part| matches!(part, Component::Normal(_)));
    (plain && root.join(&main).is_file()).then_some(main)
}

/// Saves the main file of the project `root`, or removes the choice if `main` is `None`.
/// The lines of other projects stay. A path with a tab or a line break cannot be saved, because
/// the format uses them as separators.
pub fn save_main(file: &Path, root: &Path, main: Option<&Path>) -> io::Result<()> {
    let value = main.map(|main| main.as_os_str().as_bytes().to_vec());
    if value.as_deref().is_some_and(|value| value.contains(&b'\t')) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a path with a tab or a line break cannot be saved",
        ));
    }
    replace_line(file, root, value)
}

/// Saves that the user chose no main file for the project `root`. The program then does not pick one by
/// itself. The value `-` is no file name that `load_main` accepts, so it reads as no main file.
pub fn save_no_main(file: &Path, root: &Path) -> io::Result<()> {
    replace_line(file, root, Some(b"-".to_vec()))
}

/// True if the file has a line for the project `root`: a main file, or the choice of no main file.
pub fn has_main_choice(file: &Path, root: &Path) -> bool {
    let Ok(bytes) = fs::read(file) else {
        return false;
    };
    bytes
        .split(|byte| *byte == b'\n')
        .filter_map(split_line)
        .any(|(saved_root, _)| saved_root == root.as_os_str().as_bytes())
}

/// The file that holds the layout of the editor, one word. The layout is a choice of the user for all
/// projects, so it has no project folder in it.
pub fn layout_file(state_file: &Path) -> PathBuf {
    state_file.with_file_name("layout")
}

/// The saved layout name, or `None` if there is none or the file is not a short word.
pub fn load_layout(file: &Path) -> Option<String> {
    let text = fs::read_to_string(file).ok()?;
    let name = text.trim();
    (!name.is_empty() && name.len() <= 16 && name.chars().all(|letter| letter.is_ascii_lowercase()))
        .then(|| name.to_string())
}

/// Saves the layout name.
pub fn save_layout(file: &Path, name: &str) -> io::Result<()> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    crate::fsutil::write_file(file, name.as_bytes())
}

/// The file that holds the spell check switch: `on` or `off`.
pub fn spell_file(state_file: &Path) -> PathBuf {
    state_file.with_file_name("spell")
}

/// The personal dictionary of the spell check: one word on each line.
pub fn spell_words_file(state_file: &Path) -> PathBuf {
    state_file.with_file_name("spell-words.txt")
}

/// The saved switch: `Some(true)` for `on`, `Some(false)` for `off`, and `None` for no file or other text.
pub fn load_switch(file: &Path) -> Option<bool> {
    match fs::read_to_string(file).ok()?.trim() {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

/// Saves the switch.
pub fn save_switch(file: &Path, on: bool) -> io::Result<()> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    crate::fsutil::write_file(file, if on { b"on" } else { b"off" })
}

/// The file that holds the last open file of each project, next to the main file list.
/// Each line is `<project folder>`, a tab, `<file>`, a tab, and the page number.
pub fn last_file(state_file: &Path) -> PathBuf {
    state_file.with_file_name("last-files")
}

/// The saved last file of the project `root` and its page, if the file still exists inside the project.
/// A missing file, a broken line, and a saved path that leaves the project all give `None`.
pub fn load_last(file: &Path, root: &Path) -> Option<(PathBuf, usize)> {
    let bytes = fs::read(file).ok()?;
    let value = bytes
        .split(|byte| *byte == b'\n')
        .filter_map(|line| {
            split_line(line).filter(|(saved_root, _)| *saved_root == root.as_os_str().as_bytes())
        })
        .map(|(_, value)| value)
        .next_back()?;
    let tab = value.iter().rposition(|byte| *byte == b'\t')?;
    let path = PathBuf::from(OsStr::from_bytes(&value[..tab]));
    let page = std::str::from_utf8(&value[tab + 1..])
        .ok()?
        .parse::<usize>()
        .ok()
        .filter(|page| *page >= 1)?;
    let plain = path
        .components()
        .all(|part| matches!(part, Component::Normal(_)));
    (plain && root.join(&path).is_file()).then_some((path, page))
}

/// Saves the last open file of the project `root` and the page of its preview.
pub fn save_last(file: &Path, root: &Path, path: &Path, page: usize) -> io::Result<()> {
    let mut value = path.as_os_str().as_bytes().to_vec();
    value.extend_from_slice(format!("\t{page}").as_bytes());
    // The path must not hold a tab: the page would then be read from the wrong place.
    if path.as_os_str().as_bytes().contains(&b'\t') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a path with a tab or a line break cannot be saved",
        ));
    }
    replace_line(file, root, Some(value))
}

/// Writes the line of the project `root` with `value` after the tab, or removes the line if `value` is
/// `None`. The lines of other projects stay.
fn replace_line(file: &Path, root: &Path, value: Option<Vec<u8>>) -> io::Result<()> {
    let root_bytes = root.as_os_str().as_bytes();
    let bad = |bytes: &[u8]| bytes.contains(&b'\n') || bytes.contains(&b'\t');
    // The value of a last file has its own tab, so only a line break is a fault there.
    if bad(root_bytes) || value.as_deref().is_some_and(|value| value.contains(&b'\n')) {
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
    for line in old
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        if split_line(line).is_none_or(|(saved_root, _)| saved_root != root_bytes) {
            new.extend_from_slice(line);
            new.push(b'\n');
        }
    }
    if let Some(value) = value {
        new.extend_from_slice(root_bytes);
        new.push(b'\t');
        new.extend_from_slice(&value);
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
        let dir =
            std::env::temp_dir().join(format!("lazytypst-state-{name}-{}", std::process::id()));
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
        assert_eq!(
            load_main(&file, &two),
            Some(PathBuf::from("chapters/one.typ"))
        );

        // A new choice replaces the old choice of the same project only.
        save_main(&file, &one, Some(Path::new("chapters/one.typ"))).unwrap();
        assert_eq!(
            load_main(&file, &one),
            Some(PathBuf::from("chapters/one.typ"))
        );
        assert_eq!(
            load_main(&file, &two),
            Some(PathBuf::from("chapters/one.typ"))
        );
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
        for bad in [
            "../outside.typ",
            "/etc/passwd",
            "chapters/../../outside.typ",
        ] {
            fs::write(&file, format!("{}\t{bad}\n", root.display())).unwrap();
            assert_eq!(load_main(&file, &root), None, "{bad}");
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_last_file_and_its_page_come_back_for_the_same_project_only() {
        let dir = temp_dir("last");
        let (one, two) = (project(&dir, "one"), project(&dir, "two"));
        let file = last_file(&dir.join("state").join("main-files"));
        assert_eq!(file, dir.join("state").join("last-files"));

        save_last(&file, &one, Path::new("chapters/one.typ"), 3).unwrap();
        save_last(&file, &two, Path::new("main.typ"), 1).unwrap();
        assert_eq!(
            load_last(&file, &one),
            Some((PathBuf::from("chapters/one.typ"), 3))
        );
        assert_eq!(load_last(&file, &two), Some((PathBuf::from("main.typ"), 1)));
        save_last(&file, &one, Path::new("main.typ"), 2).unwrap();
        assert_eq!(load_last(&file, &one), Some((PathBuf::from("main.typ"), 2)));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_deleted_or_broken_last_file_gives_none_and_no_error() {
        let dir = temp_dir("lastgone");
        let root = project(&dir, "p");
        let file = dir.join("last-files");
        assert_eq!(load_last(&file, &root), None, "no state file");
        save_last(&file, &root, Path::new("main.typ"), 2).unwrap();
        fs::remove_file(root.join("main.typ")).unwrap();
        assert_eq!(load_last(&file, &root), None, "the file is gone");
        for bad in ["main.typ\t0", "main.typ\tx", "main.typ", "../x.typ\t1"] {
            let line = format!("{}\t{bad}\n", root.display());
            fs::write(&file, line).unwrap();
            fs::write(root.join("main.typ"), "").unwrap();
            assert_eq!(load_last(&file, &root), None, "{bad}");
        }
        fs::remove_dir_all(&dir).unwrap();
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
    #[test]
    fn the_choice_of_no_main_file_is_saved_and_a_removed_choice_is_not() {
        let dir = temp_dir("nomain");
        let root = project(&dir, "p");
        let file = dir.join("main-files");
        assert!(!has_main_choice(&file, &root), "no file");
        save_main(&file, &root, Some(Path::new("main.typ"))).unwrap();
        assert!(has_main_choice(&file, &root));
        save_no_main(&file, &root).unwrap();
        assert!(has_main_choice(&file, &root), "no main file is a choice");
        assert_eq!(load_main(&file, &root), None);
        save_main(&file, &root, None).unwrap();
        assert!(
            !has_main_choice(&file, &root),
            "a removed line is no choice"
        );
        assert!(!has_main_choice(&file, &dir.join("other")));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_layout_name_is_saved_and_a_bad_file_gives_none() {
        let dir = temp_dir("layout");
        let file = layout_file(&dir.join("main-files"));
        assert_eq!(load_layout(&file), None);
        save_layout(&file, "stacked").unwrap();
        assert_eq!(load_layout(&file).as_deref(), Some("stacked"));
        fs::write(&file, "Not A Word\n").unwrap();
        assert_eq!(load_layout(&file), None);
        fs::write(&file, "x".repeat(100)).unwrap();
        assert_eq!(load_layout(&file), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_switch_is_saved_and_other_text_gives_none() {
        let dir = temp_dir("switch");
        let file = spell_file(&dir.join("main-files"));
        assert_eq!(load_switch(&file), None);
        save_switch(&file, true).unwrap();
        assert_eq!(load_switch(&file), Some(true));
        save_switch(&file, false).unwrap();
        assert_eq!(load_switch(&file), Some(false));
        fs::write(&file, "maybe").unwrap();
        assert_eq!(load_switch(&file), None);
        fs::remove_dir_all(&dir).unwrap();
    }
}
