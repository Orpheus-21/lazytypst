use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// How many folder levels below the root the browser searches.
pub const MAX_DEPTH: usize = 3;

/// Returns the `.typ` files under `root` as sorted paths relative to `root`.
/// Hidden folders and hidden files are skipped. Only regular files count. A link counts only if it points
/// at a regular file inside the root: a project from the internet can hold `x.typ` as a link to a file of
/// the user, or to a device, or to a pipe that never ends.
pub fn find_typ_files(root: &Path, max_depth: usize) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let canonical_root = fs::canonicalize(root)?;
    walk(root, &canonical_root, Path::new(""), max_depth, &mut found)?;
    found.sort();
    Ok(found)
}

fn walk(
    root: &Path,
    canonical_root: &Path,
    rel: &Path,
    depth_left: usize,
    found: &mut Vec<PathBuf>,
) -> io::Result<()> {
    for entry in fs::read_dir(root.join(rel))?.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = rel.join(&name);
        if kind.is_dir() {
            if depth_left > 0 {
                // A folder that cannot be read is skipped. Only the root can fail.
                let _ = walk(root, canonical_root, &path, depth_left - 1, found);
            }
        } else if path.extension().is_some_and(|ext| ext == "typ")
            && (kind.is_file()
                || (kind.is_symlink() && link_stays_inside(canonical_root, &root.join(&path))))
        {
            found.push(path);
        }
    }
    Ok(())
}

/// True if the link `path` points at a regular file inside `canonical_root`.
fn link_stays_inside(canonical_root: &Path, path: &Path) -> bool {
    fs::canonicalize(path)
        .is_ok_and(|target| target.starts_with(canonical_root) && target.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_typ_files_to_the_depth_limit() {
        let root = std::env::temp_dir().join(format!("lazytypst-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for file in [
            "a.typ",
            "notes.txt",
            "sub/b.typ",
            "sub/c.txt",
            ".hidden/d.typ",
            "l1/l2/l3/ok.typ",
            "l1/l2/l3/l4/deep.typ",
        ] {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }

        let found = find_typ_files(&root, MAX_DEPTH).unwrap();
        fs::remove_dir_all(&root).unwrap();

        let expected: Vec<PathBuf> = ["a.typ", "l1/l2/l3/ok.typ", "sub/b.typ"]
            .into_iter()
            .map(PathBuf::from)
            .collect();
        assert_eq!(found, expected);
    }

    #[test]
    fn a_missing_root_is_an_error() {
        assert!(find_typ_files(Path::new("/no/such/folder"), MAX_DEPTH).is_err());
    }
    #[test]
    fn a_link_to_a_file_outside_the_root_and_a_pipe_are_not_listed() {
        let base = std::env::temp_dir().join(format!("lazytypst-links-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("project");
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(base.join("secret.txt"), "x").unwrap();
        fs::write(root.join("real.typ"), "").unwrap();
        fs::write(root.join("sub/inner.typ"), "").unwrap();
        let link = |target: &Path, name: &str| {
            std::os::unix::fs::symlink(target, root.join(name)).unwrap();
        };
        link(&base.join("secret.txt"), "outside.typ");
        link(Path::new("/dev/zero"), "device.typ");
        link(Path::new("/no/such/file"), "dangling.typ");
        link(&root.join("sub/inner.typ"), "inside.typ");
        link(&root.join("sub"), "folder.typ");
        let status = std::process::Command::new("mkfifo")
            .arg(root.join("pipe.typ"))
            .status()
            .unwrap();
        assert!(status.success());

        let found = find_typ_files(&root, MAX_DEPTH).unwrap();
        fs::remove_dir_all(&base).unwrap();
        let expected: Vec<PathBuf> = ["inside.typ", "real.typ", "sub/inner.typ"]
            .into_iter()
            .map(PathBuf::from)
            .collect();
        assert_eq!(found, expected);
    }
}
