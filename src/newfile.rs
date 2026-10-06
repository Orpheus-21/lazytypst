//! Makes a new `.typ` file inside the project folder, from a name that the user typed.

use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

/// Makes the file `name` inside `root`, with the ending `.typ`, and returns its path relative to `root`.
///
/// The name is a path of folders and a file name, for example `chapters/two`. The file is made empty.
/// A name is refused, with a message for the user, when it would:
/// - leave the project (`..`, or a path that starts with `/`),
/// - end with something other than `.typ`,
/// - start a folder or the file with a dot, or lie deeper than `max_depth` folders, because then the
///   list of the program would not show the file,
/// - go through a folder that is a symlink, or through a file,
/// - name a path that exists already.
///
/// The file is made with `create_new`, which refuses every existing path, also a dangling symlink.
pub fn create(root: &Path, name: &str, max_depth: usize) -> Result<PathBuf, String> {
    let name = name.trim();
    if name.ends_with('/') {
        return Err("Type a file name, not a folder.".into());
    }
    let mut parts = Vec::new();
    for part in Path::new(name).components() {
        match part {
            Component::Normal(part) => parts.push(part.to_owned()),
            Component::CurDir => {}
            _ => return Err("The name must stay inside the project. Do not use .. or a first /.".into()),
        }
    }
    let Some(file_name) = parts.pop() else {
        return Err("Type a file name.".into());
    };
    if parts.iter().chain([&file_name]).any(|part| part.to_string_lossy().starts_with('.')) {
        return Err("A name that starts with a dot is not shown in the list. Choose another name.".into());
    }
    if parts.len() + 1 > max_depth + 1 {
        return Err(format!("The file would be deeper than the list reads ({max_depth} folders)."));
    }
    let mut file_name = file_name;
    match Path::new(&file_name).extension() {
        None => file_name.push(".typ"),
        Some(ending) if ending == "typ" => {}
        Some(_) => return Err("The name must end with .typ, or have no ending.".into()),
    }

    let mut folder = root.to_path_buf();
    let mut relative = PathBuf::new();
    for part in &parts {
        folder.push(part);
        relative.push(part);
        match fs::symlink_metadata(&folder) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(format!("{} is a link. lazytypst does not make files through a link.", relative.display()));
            }
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(format!("{} is not a folder.", relative.display())),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&folder).map_err(|err| format!("Cannot make the folder {}: {err}", relative.display()))?;
            }
            Err(err) => return Err(format!("Cannot read {}: {err}", relative.display())),
        }
    }
    relative.push(&file_name);
    match fs::OpenOptions::new().write(true).create_new(true).open(root.join(&relative)) {
        Ok(_) => Ok(relative),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Err(format!("{} exists already.", relative.display())),
        Err(err) => Err(format!("Cannot make {}: {err}", relative.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::MAX_DEPTH;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("lazytypst-new-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn a_name_without_an_ending_gets_the_ending_typ() {
        let root = temp_root("ending");
        assert_eq!(create(&root, "notes", MAX_DEPTH).unwrap(), PathBuf::from("notes.typ"));
        assert_eq!(fs::read(root.join("notes.typ")).unwrap(), b"");
        assert_eq!(create(&root, "other.typ", MAX_DEPTH).unwrap(), PathBuf::from("other.typ"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_folders_are_made() {
        let root = temp_root("folders");
        assert_eq!(create(&root, "chapters/part/two", MAX_DEPTH).unwrap(), PathBuf::from("chapters/part/two.typ"));
        assert!(root.join("chapters").join("part").join("two.typ").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn spaces_around_the_name_and_a_leading_dot_slash_are_dropped() {
        let root = temp_root("trim");
        assert_eq!(create(&root, "  ./a/b  ", MAX_DEPTH).unwrap(), PathBuf::from("a/b.typ"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn two_dots_inside_a_name_are_fine_and_a_dot_makes_an_ending() {
        let root = temp_root("dots");
        assert_eq!(create(&root, "a..b.typ", MAX_DEPTH).unwrap(), PathBuf::from("a..b.typ"));
        assert!(create(&root, "a..b", MAX_DEPTH).unwrap_err().contains(".typ"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_existing_file_is_not_changed() {
        let root = temp_root("exists");
        fs::write(root.join("notes.typ"), "keep me").unwrap();
        let err = create(&root, "notes", MAX_DEPTH).unwrap_err();
        assert!(err.contains("exists already"), "{err}");
        assert_eq!(fs::read_to_string(root.join("notes.typ")).unwrap(), "keep me");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_path_that_leaves_the_project_is_refused() {
        let root = temp_root("leave");
        let outside = root.parent().unwrap().join(format!("lazytypst-new-outside-{}.typ", std::process::id()));
        for bad in ["../x", "a/../../x", "/tmp/x", "a/../b"] {
            let err = create(&root, bad, MAX_DEPTH).unwrap_err();
            assert!(err.contains("inside the project"), "{bad}: {err}");
        }
        assert!(!outside.exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0, "a folder or file was made");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_ending_other_than_typ_is_refused() {
        let root = temp_root("other");
        for bad in ["two.md", "two.TYP", "x.typ.bak"] {
            let err = create(&root, bad, MAX_DEPTH).unwrap_err();
            assert!(err.contains(".typ"), "{bad}: {err}");
        }
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_hidden_name_is_refused_because_the_list_does_not_show_it() {
        let root = temp_root("hidden");
        for bad in [".draft", ".git/x", "a/.b/c"] {
            let err = create(&root, bad, MAX_DEPTH).unwrap_err();
            assert!(err.contains("dot"), "{bad}: {err}");
        }
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_file_deeper_than_the_list_reads_is_refused() {
        let root = temp_root("deep");
        assert!(create(&root, "a/b/c/ok", MAX_DEPTH).is_ok());
        let err = create(&root, "a/b/c/d/deep", MAX_DEPTH).unwrap_err();
        assert!(err.contains("deeper"), "{err}");
        assert!(!root.join("a").join("b").join("c").join("d").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_empty_name_and_a_folder_name_are_refused() {
        let root = temp_root("empty");
        for bad in ["", "   ", "chapters/", "./", "."] {
            assert!(create(&root, bad, MAX_DEPTH).is_err(), "{bad:?}");
        }
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_folder_that_is_a_link_is_not_used() {
        let root = temp_root("link");
        let elsewhere = temp_root("link-target");
        std::os::unix::fs::symlink(&elsewhere, root.join("link")).unwrap();
        for bad in ["link/x", "link/newdir/x"] {
            let err = create(&root, bad, MAX_DEPTH).unwrap_err();
            assert!(err.contains("link"), "{bad}: {err}");
        }
        assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0, "a file or folder was made through the link");
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(elsewhere).unwrap();
    }

    #[test]
    fn a_link_at_the_file_name_is_not_followed() {
        let root = temp_root("filelink");
        let elsewhere = temp_root("filelink-target");
        std::os::unix::fs::symlink(elsewhere.join("y.typ"), root.join("x.typ")).unwrap(); // a dangling link
        assert!(create(&root, "x", MAX_DEPTH).is_err());
        assert!(!elsewhere.join("y.typ").exists(), "the link target was made");
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(elsewhere).unwrap();
    }

    #[test]
    fn a_file_in_the_place_of_a_folder_is_refused() {
        let root = temp_root("notdir");
        fs::write(root.join("a.typ"), "").unwrap();
        let err = create(&root, "a.typ/x", MAX_DEPTH).unwrap_err();
        assert!(err.contains("not a folder"), "{err}");
        fs::remove_dir_all(root).unwrap();
    }
}
