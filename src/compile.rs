use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
};

/// The result of one `typst compile` run.
pub struct Report {
    pub ok: bool,
    /// The lines that `typst` printed on stderr. Errors and warnings are here.
    pub lines: Vec<String>,
}

impl Report {
    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            lines: vec![message.into()],
        }
    }
}

/// The folder that holds the PNG pages of this run.
pub fn out_dir() -> PathBuf {
    std::env::temp_dir().join(format!("lazytypst-{}", std::process::id()))
}

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

/// A new folder inside `out_dir` for the pages of one editor.
/// A compile that outlives its editor then cannot write into the pages of the next editor.
pub fn new_out_dir() -> PathBuf {
    out_dir().join(NEXT_DIR.fetch_add(1, Ordering::Relaxed).to_string())
}

/// Deletes the page folder. The program calls this when it exits.
pub fn cleanup() {
    let _ = fs::remove_dir_all(out_dir());
}

/// Runs `typst compile` in a thread. The report arrives on the returned channel.
pub fn start(file: PathBuf, out_dir: PathBuf) -> Receiver<Report> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        // The receiver is gone when the user closed the editor. Then nobody needs the report.
        let _ = tx.send(run(&file, &out_dir));
    });
    rx
}

/// Compiles `file` to `out_dir/page-{p}.png` and waits for the result.
pub fn run(file: &Path, out_dir: &Path) -> Report {
    if let Err(err) = fs::create_dir_all(out_dir) {
        return Report::failed(format!("Cannot make {}: {err}", out_dir.display()));
    }
    let result = Command::new("typst")
        .args(["compile", "--format", "png", "--diagnostic-format", "short"])
        .arg(file)
        .arg(out_dir.join("page-{p}.png"))
        .output();
    match result {
        Ok(output) => {
            let ok = output.status.success();
            let mut lines: Vec<String> = String::from_utf8_lossy(&output.stderr)
                .lines()
                .map(String::from)
                .collect();
            if !ok && lines.is_empty() {
                lines.push(format!("typst failed: {}", output.status));
            }
            Report { ok, lines }
        }
        Err(err) => Report::failed(format!("Cannot run typst: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Makes a `doc.typ` with `content` in a new temporary folder. Returns the file and a page folder.
    fn project(name: &str, content: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("lazytypst-compile-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("doc.typ");
        fs::write(&file, content).unwrap();
        (file, dir.join("pages"))
    }

    #[test]
    fn a_valid_file_makes_a_png_page() {
        let (file, pages) = project("ok", "= Title\nSome text.\n");
        let report = run(&file, &pages);
        assert!(report.ok, "{:?}", report.lines);
        assert!(report.lines.is_empty());
        let png = fs::read(pages.join("page-1.png")).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_error_report_names_the_line() {
        let (file, pages) = project("bad", "= Title\n#nope()\n");
        let report = run(&file, &pages);
        assert!(!report.ok);
        assert!(
            report.lines.iter().any(|l| l.contains(":2:") && l.contains("error")),
            "{:?}",
            report.lines
        );
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn each_new_out_dir_is_different_and_inside_out_dir() {
        let (first, second) = (new_out_dir(), new_out_dir());
        assert_ne!(first, second);
        assert_eq!(first.parent(), Some(out_dir().as_path()));
    }

    #[test]
    fn a_missing_input_file_is_a_failure() {
        let (file, pages) = project("missing", "");
        fs::remove_file(&file).unwrap();
        let report = run(&file, &pages);
        assert!(!report.ok);
        assert!(!report.lines.is_empty());
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }
}
