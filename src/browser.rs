use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// How many folder levels below the root the browser searches.
pub const MAX_DEPTH: usize = 3;

/// Returns the `.typ` files under `root` as sorted paths relative to `root`.
/// Hidden folders and hidden files are skipped.
pub fn find_typ_files(root: &Path, max_depth: usize) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    walk(root, Path::new(""), max_depth, &mut found)?;
    found.sort();
    Ok(found)
}

fn walk(root: &Path, rel: &Path, depth_left: usize, found: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(root.join(rel))?.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else { continue };
        let path = rel.join(&name);
        if kind.is_dir() {
            if depth_left > 0 {
                // A folder that cannot be read is skipped. Only the root can fail.
                let _ = walk(root, &path, depth_left - 1, found);
            }
        } else if path.extension().is_some_and(|ext| ext == "typ") {
            found.push(path);
        }
    }
    Ok(())
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
}
