use std::{
    fs, io,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
};

/// The result of one `typst compile` run.
pub struct Report {
    pub ok: bool,
    /// The lines that `typst` printed on stderr. Errors and warnings are here.
    pub lines: Vec<String>,
    /// One entry for each line of `lines`, parsed once when the report is made.
    pub diagnostics: Vec<Diagnostic>,
}

impl Report {
    pub fn new(ok: bool, lines: Vec<String>) -> Self {
        let diagnostics = lines.iter().map(|line| Diagnostic::parse(line)).collect();
        Self { ok, lines, diagnostics }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self::new(false, vec![message.into()])
    }

    /// The first error that has a position in a file.
    pub fn first_error(&self) -> Option<&Diagnostic> {
        self.diagnostics
            .iter()
            .find(|diagnostic| diagnostic.severity == Severity::Error && diagnostic.file.is_some())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    /// A line that is not an error or a warning with a position, for example a hint or a message of lazytypst.
    Other,
}

/// One line of the output of `typst compile --diagnostic-format short`:
/// `<file>:<line>:<column>: error: <message>`, or the same with `warning`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    /// The file as Typst prints it. Typst runs in the project root, so this is a path relative to the root.
    pub file: Option<PathBuf>,
    /// The line, counted from 1. It is 0 when the line has no position.
    pub line: usize,
    /// The column, counted from 1 in characters (Unicode scalar values). It is 0 when the line has no position.
    pub column: usize,
    /// The text after `error: ` or `warning: `. For `Other`, the whole line.
    pub message: String,
}

impl Diagnostic {
    /// Parses one line. A line that does not have the form above becomes `Other` with the whole line as message.
    /// The first marker `: error: ` or `: warning: ` splits the line, so a colon or a marker inside the message
    /// is safe. The part before it is split from the right, so a colon inside the path is safe too.
    pub fn parse(line: &str) -> Diagnostic {
        let other = || Diagnostic {
            severity: Severity::Other,
            file: None,
            line: 0,
            column: 0,
            message: line.to_string(),
        };
        let found = [(": error: ", Severity::Error), (": warning: ", Severity::Warning)]
            .into_iter()
            .filter_map(|(marker, severity)| line.find(marker).map(|at| (at, marker, severity)))
            .min_by_key(|(at, _, _)| *at);
        let Some((at, marker, severity)) = found else {
            return other();
        };
        let mut head = line[..at].rsplitn(3, ':');
        let (Some(column), Some(row), Some(file)) = (head.next(), head.next(), head.next()) else {
            return other();
        };
        let (Ok(column), Ok(row)) = (column.parse::<usize>(), row.parse::<usize>()) else {
            return other();
        };
        if file.is_empty() {
            return other();
        }
        Diagnostic {
            severity,
            file: Some(PathBuf::from(file)),
            line: row,
            column,
            message: line[at + marker.len()..].to_string(),
        }
    }
}

/// The folder that holds the PNG pages of this run.
pub fn out_dir() -> PathBuf {
    std::env::temp_dir().join(format!("lazytypst-{}", std::process::id()))
}

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

/// A new folder name inside `root` for the pages of one compile.
/// No two compiles share a folder. So a compile that was killed cannot add pages to a newer one.
pub fn next_dir(root: &Path) -> PathBuf {
    root.join(NEXT_DIR.fetch_add(1, Ordering::Relaxed).to_string())
}

/// Makes the page folder of this run. Only the user can open it.
/// The call fails if the path exists already, also as a symlink. So the program never writes
/// into a folder that another user prepared, and the cleanup never deletes files of another user.
/// One case is not a failure: see `make_out_dir_for`.
pub fn make_out_dir() -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    // The owner of /proc/self is the user of this process.
    make_out_dir_for(&out_dir(), fs::metadata("/proc/self")?.uid())
}

/// Makes the folder `path` for the user `own_uid`. If `path` exists already, it is deleted and made again
/// when it is a real folder of that user. The name of the folder holds the pid of this process, and no
/// other process has this pid. So a folder that is already there was left by an earlier run whose pid the
/// system has used again. A symlink, a file, and a folder of another user still stop the call with the
/// error `AlreadyExists`. (This holds if the processes that share the temporary directory share one
/// pid space.)
fn make_out_dir_for(path: &Path, own_uid: u32) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let exists = match make_private_dir(path) {
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => err,
        other => return other,
    };
    // `symlink_metadata` does not follow a link, so a link is never taken for a folder.
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && meta.uid() == own_uid => {
            fs::remove_dir_all(path)?;
            make_private_dir(path)
        }
        _ => Err(exists),
    }
}

fn make_private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().mode(0o700).create(path)
}

/// Deletes the page folders of earlier runs that ended without a cleanup, for example after a crash
/// or a closed terminal window. Call it after `make_out_dir`: the owner of that new folder is this user.
pub fn remove_stale_dirs() {
    use std::os::unix::fs::MetadataExt;
    if let Ok(own) = fs::metadata(out_dir()) {
        remove_stale_dirs_in(&std::env::temp_dir(), own.uid());
    }
}

/// Deletes each folder `lazytypst-<pid>` in `tmp` that is a real folder (not a symlink),
/// belongs to the user `uid`, and has no running process with that pid.
fn remove_stale_dirs_in(tmp: &Path, uid: u32) {
    use std::os::unix::fs::MetadataExt;
    let Ok(entries) = fs::read_dir(tmp) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|name| name.strip_prefix("lazytypst-")) else {
            continue;
        };
        if pid.is_empty() || !pid.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if meta.is_dir() && meta.uid() == uid && !Path::new("/proc").join(pid).exists() {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Deletes the page folder. The program calls this when it exits.
pub fn cleanup() {
    let _ = fs::remove_dir_all(out_dir());
}

/// The first line that `<program> --version` prints, for example `typst 0.15.1 (9dfd3a08)`.
/// The error has the kind `NotFound` when the program is not in `PATH`.
pub fn version_of(program: &str) -> io::Result<String> {
    let output = Command::new(program)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!("{program} --version failed: {}", output.status)));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(String::from)
        .ok_or_else(|| io::Error::other(format!("{program} --version printed nothing")))
}

/// The version line of the `typst` command. See `version_of`.
pub fn typst_version() -> io::Result<String> {
    version_of("typst")
}

/// A running command. Dropping the job kills the process, so a new job can replace an old one.
pub struct Job {
    state: State,
    /// What the command writes: a folder of PNG pages, or one PDF file.
    output: PathBuf,
}

enum State {
    /// The command did not start. The report waits here until `try_report` takes it.
    Failed(Option<Report>),
    /// The command runs. A thread reads its stderr and sends the bytes when the process closes stderr.
    Running { child: Child, stderr: Receiver<Vec<u8>> },
}

impl Job {
    /// Runs `typst compile` on `file` for page number `page` only, counted from 1.
    /// The one file goes to `dir/page-<page>-of-<count>.png`, where `<count>` is the number of pages of the
    /// document. If the document has fewer than `page` pages, Typst exits with success and writes no file.
    /// Typst can read the files under `root`. So `file` can import files from parent folders inside `root`.
    /// Typst runs in `root`, so an error line names the file relative to `root`.
    /// `file` and `dir` must be absolute, or relative to `root`.
    pub fn start(file: &Path, root: &Path, dir: PathBuf, page: usize) -> Job {
        if let Err(err) = fs::create_dir_all(&dir) {
            return Job::failed(format!("Cannot make {}: {err}", dir.display()), dir);
        }
        let mut command = Command::new("typst");
        command
            .current_dir(root)
            .args(["compile", "--format", "png", "--diagnostic-format", "short", "--root"])
            .arg(root)
            .arg("--pages")
            .arg(page.to_string())
            .arg(file)
            .arg(dir.join("page-{p}-of-{t}.png"));
        Job::spawn(command, dir)
    }

    /// Runs `typst compile` on `file`. The PDF goes to `pdf`. An old file at `pdf` is replaced.
    /// The paths follow the same rules as in `start`.
    pub fn start_pdf(file: &Path, root: &Path, pdf: PathBuf) -> Job {
        let mut command = Command::new("typst");
        command
            .current_dir(root)
            .args(["compile", "--format", "pdf", "--diagnostic-format", "short", "--root"])
            .arg(root)
            .arg(file)
            .arg(&pdf);
        Job::spawn(command, pdf)
    }

    /// A job that ends at once with success and writes nothing. For tests only.
    #[cfg(test)]
    pub fn ended_with_success(output: PathBuf) -> Job {
        Job::spawn(Command::new("true"), output)
    }

    /// What the command writes: the page folder of `start`, or the PDF file of `start_pdf`.
    pub fn output(&self) -> &Path {
        &self.output
    }

    fn failed(message: String, output: PathBuf) -> Job {
        Job { state: State::Failed(Some(Report::failed(message))), output }
    }

    /// Starts the command. The lines it prints on stderr become the report.
    fn spawn(mut command: Command, output: PathBuf) -> Job {
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                let program = command.get_program().to_string_lossy().into_owned();
                return Job::failed(format!("Cannot run {program}: {err}"), output);
            }
        };
        let mut pipe = child.stderr.take().expect("stderr is piped");
        let (tx, stderr) = mpsc::channel();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            // The receiver is gone when the job was dropped. Then nobody needs the bytes.
            let _ = tx.send(bytes);
        });
        Job { state: State::Running { child, stderr }, output }
    }

    /// Returns the report if the command has ended. Never waits.
    pub fn try_report(&mut self) -> Option<Report> {
        let (child, stderr) = match &mut self.state {
            State::Failed(report) => return report.take(),
            State::Running { child, stderr } => (child, stderr),
        };
        let status = match child.try_wait() {
            Ok(None) => return None,
            Ok(Some(status)) => status,
            Err(err) => return Some(report(Err(err), &[])),
        };
        match stderr.try_recv() {
            Ok(bytes) => Some(report(Ok(status), &bytes)),
            // The process has ended, but the thread has not sent the bytes yet. The next call gets them.
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(report(Ok(status), &[])),
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if let State::Running { child, .. } = &mut self.state {
            // The process may have ended already. Then `kill` returns an error that does not matter.
            let _ = child.kill();
            // `wait` removes the dead process from the process table. After a kill it returns at once.
            let _ = child.wait();
        }
    }
}

fn report(status: io::Result<ExitStatus>, stderr: &[u8]) -> Report {
    let mut lines: Vec<String> = String::from_utf8_lossy(stderr).lines().map(String::from).collect();
    match status {
        Ok(status) => {
            let ok = status.success();
            if !ok && lines.is_empty() {
                lines.push(format!("typst failed: {status}"));
            }
            Report::new(ok, lines)
        }
        Err(err) => Report::failed(format!("Cannot wait for typst: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Waits up to 10 seconds for the report of the job.
    fn wait(job: &mut Job) -> Report {
        let start = Instant::now();
        loop {
            if let Some(report) = job.try_report() {
                return report;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "the job did not end");
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Makes a `doc.typ` with `content` in a new temporary folder. Returns the file and a page folder.
    fn project(name: &str, content: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("lazytypst-compile-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("doc.typ");
        fs::write(&file, content).unwrap();
        (file, dir.join("pages"))
    }

    fn parse(line: &str) -> Diagnostic {
        Diagnostic::parse(line)
    }

    fn at(file: &str, line: usize, column: usize, severity: Severity, message: &str) -> Diagnostic {
        Diagnostic { severity, file: Some(PathBuf::from(file)), line, column, message: message.into() }
    }

    #[test]
    fn an_error_line_is_parsed() {
        assert_eq!(
            parse("report.typ:12:5: error: unknown variable: x"),
            at("report.typ", 12, 5, Severity::Error, "unknown variable: x")
        );
    }

    #[test]
    fn a_warning_line_is_parsed() {
        assert_eq!(
            parse("w.typ:1:16: warning: unknown font family: nosuchfont"),
            at("w.typ", 1, 16, Severity::Warning, "unknown font family: nosuchfont")
        );
    }

    #[test]
    fn a_path_with_spaces_or_a_colon_is_parsed() {
        assert_eq!(
            parse("sp ace/a b.typ:1:1: error: unknown variable: nope"),
            at("sp ace/a b.typ", 1, 1, Severity::Error, "unknown variable: nope")
        );
        assert_eq!(
            parse("co:lon/x.typ:3:2: error: boom"),
            at("co:lon/x.typ", 3, 2, Severity::Error, "boom")
        );
    }

    #[test]
    fn a_message_with_colons_and_markers_stays_whole() {
        assert_eq!(
            parse("a.typ:2:3: error: expected: error: found: warning: x"),
            at("a.typ", 2, 3, Severity::Error, "expected: error: found: warning: x")
        );
    }

    #[test]
    fn a_line_that_is_not_a_diagnostic_stays_as_it_is() {
        for line in [
            "hint: try adding a space",
            "error: the input file was not found",
            "Cannot run typst: No such file or directory",
            "a.typ:x:2: error: not a number",
            ":1:2: error: no file",
            "",
        ] {
            let d = parse(line);
            assert_eq!(d.severity, Severity::Other, "{line:?}");
            assert_eq!(d.file, None, "{line:?}");
            assert_eq!(d.message, line, "{line:?}");
        }
    }

    #[test]
    fn a_report_parses_each_line_once_and_keeps_the_text() {
        let report = Report::new(false, vec!["a.typ:1:2: error: boom".into(), "hint: x".into()]);
        assert_eq!(report.lines, ["a.typ:1:2: error: boom", "hint: x"]);
        assert_eq!(report.diagnostics.len(), 2);
        assert_eq!(report.diagnostics[0].severity, Severity::Error);
        assert_eq!(report.diagnostics[1].severity, Severity::Other);
        assert_eq!(Report::failed("Cannot run typst").diagnostics[0].severity, Severity::Other);
    }

    #[test]
    fn the_first_error_skips_warnings_and_lines_without_a_position() {
        let report = Report::new(
            false,
            vec![
                "hint: first".into(),
                "a.typ:1:1: warning: w".into(),
                "b.typ:4:7: error: e1".into(),
                "c.typ:9:9: error: e2".into(),
            ],
        );
        assert_eq!(report.first_error(), Some(&at("b.typ", 4, 7, Severity::Error, "e1")));
        assert_eq!(Report::new(true, vec!["a.typ:1:1: warning: w".into()]).first_error(), None);
        assert_eq!(Report::new(true, vec![]).first_error(), None);
    }

    #[test]
    fn a_real_typst_error_gets_a_position() {
        let (file, pages) = project("diag", "= Title\n#nope()\n");
        let report = wait(&mut Job::start(&file, file.parent().unwrap(), pages.clone(), 1));
        let first = report.first_error().expect("an error with a position");
        assert_eq!((first.line, first.column), (2, 1), "{first:?}");
        assert!(first.message.contains("unknown variable"), "{first:?}");
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn typst_counts_the_column_in_characters_from_1() {
        for (text, column) in [("é #nope()", 3), ("संस्कृतम् #nope()", 11), ("👨\u{200d}👩\u{200d}👧\u{200d}👦 #nope()", 9)] {
            let (file, pages) = project("column", &format!("{text}\n"));
            let report = wait(&mut Job::start(&file, file.parent().unwrap(), pages.clone(), 1));
            let first = report.first_error().expect("an error with a position");
            assert_eq!(first.column, column, "{text}");
            fs::remove_dir_all(file.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn a_valid_file_makes_a_png_page() {
        let (file, pages) = project("ok", "= Title\nSome text.\n");
        let report = wait(&mut Job::start(&file, file.parent().unwrap(), pages.clone(), 1));
        assert!(report.ok, "{:?}", report.lines);
        assert!(report.lines.is_empty());
        let png = fs::read(pages.join("page-1-of-1.png")).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_file_in_a_subfolder_can_import_a_file_from_the_root() {
        let (file, pages) = project("root", "");
        let root = file.parent().unwrap();
        fs::write(root.join("lib.typ"), "#let title = \"Lib\"\n").unwrap();
        fs::create_dir_all(root.join("chapters")).unwrap();
        let chapter = root.join("chapters").join("c.typ");
        fs::write(&chapter, "#import \"../lib.typ\": title\n= #title\n").unwrap();

        let report = wait(&mut Job::start(&chapter, root, pages.clone(), 1));
        assert!(report.ok, "{:?}", report.lines);
        let report = wait(&mut Job::start_pdf(&chapter, root, root.join("c.pdf")));
        assert!(report.ok, "{:?}", report.lines);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_error_path_is_relative_to_the_root() {
        let (file, pages) = project("relpath", "");
        let root = file.parent().unwrap();
        fs::create_dir_all(root.join("chapters")).unwrap();
        let chapter = root.join("chapters").join("c.typ");
        fs::write(&chapter, "#nope()\n").unwrap();

        let report = wait(&mut Job::start(&chapter, root, pages.clone(), 1));
        assert!(report.lines[0].starts_with("chapters/c.typ:1:"), "{:?}", report.lines);
        let report = wait(&mut Job::start_pdf(&chapter, root, root.join("c.pdf")));
        assert!(report.lines[0].starts_with("chapters/c.typ:1:"), "{:?}", report.lines);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_compile_of_one_page_writes_exactly_one_png_named_with_the_page_count() {
        let (file, pages) = project("onepage", "= One\n#pagebreak()\n= Two\n#pagebreak()\n= Three\n");
        let report = wait(&mut Job::start(&file, file.parent().unwrap(), pages.clone(), 2));
        assert!(report.ok, "{:?}", report.lines);
        let names: Vec<_> = fs::read_dir(&pages).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["page-2-of-3.png"]);
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_page_beyond_the_end_gives_success_and_no_file() {
        let (file, pages) = project("beyond", "= One\n#pagebreak()\n= Two\n");
        let report = wait(&mut Job::start(&file, file.parent().unwrap(), pages.clone(), 5));
        assert!(report.ok, "{:?}", report.lines);
        assert_eq!(fs::read_dir(&pages).unwrap().count(), 0, "typst wrote a file");
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_pdf_job_writes_a_pdf_file() {
        let (file, pages) = project("pdf", "= Title\nSome text.\n");
        let pdf = pages.with_file_name("out.pdf");
        let mut job = Job::start_pdf(&file, file.parent().unwrap(), pdf.clone());
        assert_eq!(job.output(), pdf);
        let report = wait(&mut job);
        assert!(report.ok, "{:?}", report.lines);
        assert_eq!(&fs::read(&pdf).unwrap()[..4], b"%PDF");
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_pdf_job_with_an_error_writes_no_file() {
        let (file, pages) = project("pdf-bad", "#nope()\n");
        let pdf = pages.with_file_name("out.pdf");
        let report = wait(&mut Job::start_pdf(&file, file.parent().unwrap(), pdf.clone()));
        assert!(!report.ok);
        assert!(report.lines.iter().any(|l| l.contains(":1:")), "{:?}", report.lines);
        assert!(!pdf.exists());
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_error_report_names_the_line() {
        let (file, pages) = project("bad", "= Title\n#nope()\n");
        let report = wait(&mut Job::start(&file, file.parent().unwrap(), pages.clone(), 1));
        assert!(!report.ok);
        assert!(
            report.lines.iter().any(|l| l.contains(":2:") && l.contains("error")),
            "{:?}",
            report.lines
        );
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    /// A new empty folder for a test, and the user id of its owner.
    fn test_dir(name: &str) -> (PathBuf, u32) {
        let dir = std::env::temp_dir().join(format!("lazytypst-ownpid-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let uid = uid_of(&dir);
        (dir, uid)
    }

    /// A program for a test: a shell script in a new folder. Returns the path of the script and its folder.
    fn fake_program(name: &str, script: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("lazytypst-fake-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let program = dir.join("typst");
        fs::write(&program, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        (program, dir)
    }

    /// `version_of`, with a retry for the error "Text file busy". Linux gives it when the script that a test
    /// has just written is still open for writing in a child that another test thread has just forked.
    fn version_of_script(program: &Path) -> io::Result<String> {
        let mut result = version_of(program.to_str().unwrap());
        for _ in 0..50 {
            match &result {
                Err(err) if err.raw_os_error() == Some(26) => {
                    thread::sleep(Duration::from_millis(20));
                    result = version_of(program.to_str().unwrap());
                }
                _ => break,
            }
        }
        result
    }

    #[test]
    fn the_version_is_the_first_line_that_the_program_prints() {
        let (program, dir) = fake_program("version", "echo 'typst 9.9.9 (abc123)'; echo 'a second line'");
        assert_eq!(version_of_script(&program).unwrap(), "typst 9.9.9 (abc123)");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_program_gets_the_argument_version() {
        let (program, dir) = fake_program("arg", r#"[ "$1" = "--version" ] && echo "got the argument" || echo "wrong: $*""#);
        assert_eq!(version_of_script(&program).unwrap(), "got the argument");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_program_that_is_not_there_gives_the_error_not_found() {
        let err = version_of("no-such-program-xyz").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn a_program_that_fails_or_prints_nothing_gives_an_error() {
        let (failing, dir) = fake_program("fails", "echo oops >&2; exit 3");
        let err = version_of_script(&failing).unwrap_err();
        assert!(err.to_string().contains("failed"), "{err}");
        fs::remove_dir_all(dir).unwrap();

        let (silent, dir) = fake_program("silent", "exit 0");
        let err = version_of_script(&silent).unwrap_err();
        assert!(err.to_string().contains("printed nothing"), "{err}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_real_typst_has_a_version() {
        let version = typst_version().expect("typst must be in PATH to run the tests");
        assert!(version.starts_with("typst "), "{version}");
    }

    #[test]
    fn a_leftover_folder_of_the_user_is_replaced_by_a_new_empty_folder() {
        use std::os::unix::fs::PermissionsExt;
        let (base, uid) = test_dir("leftover");
        let path = base.join("lazytypst-123");
        fs::create_dir_all(path.join("0").join("deeper")).unwrap();
        fs::write(path.join("0").join("page-1-of-1.png"), "old").unwrap();
        fs::write(path.join("old-file"), "old").unwrap();

        make_out_dir_for(&path, uid).unwrap();
        assert_eq!(fs::read_dir(&path).unwrap().count(), 0, "the folder must be new and empty");
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o700);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_missing_folder_is_made() {
        let (base, uid) = test_dir("missing");
        let path = base.join("lazytypst-123");
        make_out_dir_for(&path, uid).unwrap();
        assert!(path.is_dir());
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_symlink_at_the_path_still_stops_the_start_and_its_target_is_unchanged() {
        let (base, uid) = test_dir("symlink");
        let target = base.join("target");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("keep"), "keep").unwrap();
        let path = base.join("lazytypst-123");
        std::os::unix::fs::symlink(&target, &path).unwrap();

        let err = make_out_dir_for(&path, uid).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(target.join("keep")).unwrap(), "keep");
        assert!(fs::symlink_metadata(&path).unwrap().file_type().is_symlink(), "the link was removed");
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_dangling_symlink_at_the_path_stops_the_start_too() {
        let (base, uid) = test_dir("dangling");
        let path = base.join("lazytypst-123");
        std::os::unix::fs::symlink(base.join("nowhere"), &path).unwrap();
        assert_eq!(make_out_dir_for(&path, uid).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert!(!base.join("nowhere").exists(), "the link target was made");
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_file_at_the_path_stops_the_start_and_stays() {
        let (base, uid) = test_dir("file");
        let path = base.join("lazytypst-123");
        fs::write(&path, "keep").unwrap();
        assert_eq!(make_out_dir_for(&path, uid).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep");
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_folder_of_another_user_stops_the_start_and_stays() {
        let (base, uid) = test_dir("other-user");
        let path = base.join("lazytypst-123");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("keep"), "keep").unwrap();
        // The folder belongs to `uid`. For the process, the current user is another one.
        assert_eq!(make_out_dir_for(&path, uid + 1).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(path.join("keep")).unwrap(), "keep");
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_private_dir_is_new_and_only_the_user_can_open_it() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("lazytypst-private-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        make_private_dir(&path).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(make_private_dir(&path).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        fs::remove_dir(&path).unwrap();
    }

    #[test]
    fn a_private_dir_is_not_made_through_a_symlink() {
        let base = std::env::temp_dir().join(format!("lazytypst-symlink-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("target")).unwrap();
        std::os::unix::fs::symlink(base.join("target"), base.join("link")).unwrap();
        assert!(make_private_dir(&base.join("link")).is_err());
        fs::remove_dir_all(&base).unwrap();
    }

    /// The pid of a process that has ended and was reaped.
    fn dead_pid() -> u32 {
        let mut child = Command::new("true").spawn().unwrap();
        child.wait().unwrap();
        child.id()
    }

    /// A new folder that stands for the temporary directory in the stale folder tests.
    fn fake_tmp(name: &str) -> PathBuf {
        let tmp = std::env::temp_dir().join(format!("lazytypst-stale-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        tmp
    }

    fn uid_of(path: &Path) -> u32 {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(path).unwrap().uid()
    }

    #[test]
    fn a_stale_folder_of_a_dead_run_is_deleted() {
        let tmp = fake_tmp("dead");
        let stale = tmp.join(format!("lazytypst-{}", dead_pid()));
        fs::create_dir_all(stale.join("0")).unwrap();
        remove_stale_dirs_in(&tmp, uid_of(&tmp));
        assert!(!stale.exists());
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_folder_of_a_running_process_stays() {
        let tmp = fake_tmp("alive");
        let live = tmp.join(format!("lazytypst-{}", std::process::id()));
        fs::create_dir_all(&live).unwrap();
        remove_stale_dirs_in(&tmp, uid_of(&tmp));
        assert!(live.exists());
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_folder_of_another_user_stays() {
        let tmp = fake_tmp("owner");
        let foreign = tmp.join(format!("lazytypst-{}", dead_pid()));
        fs::create_dir_all(&foreign).unwrap();
        remove_stale_dirs_in(&tmp, uid_of(&tmp) + 1);
        assert!(foreign.exists());
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_symlink_and_other_names_stay() {
        let tmp = fake_tmp("names");
        fs::create_dir_all(tmp.join("target").join("keep")).unwrap();
        let link = tmp.join(format!("lazytypst-{}", dead_pid()));
        std::os::unix::fs::symlink(tmp.join("target"), &link).unwrap();
        let other = tmp.join(format!("lazytypst-notes-{}", dead_pid()));
        let plus = tmp.join(format!("lazytypst-+{}", dead_pid()));
        fs::create_dir_all(&other).unwrap();
        fs::create_dir_all(&plus).unwrap();

        remove_stale_dirs_in(&tmp, uid_of(&tmp));
        assert!(fs::symlink_metadata(&link).is_ok(), "the symlink was deleted");
        assert!(tmp.join("target").join("keep").exists(), "the symlink target was emptied");
        assert!(other.exists() && plus.exists());
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn each_next_dir_is_different_and_inside_the_root() {
        let root = Path::new("/some/root");
        let (first, second) = (next_dir(root), next_dir(root));
        assert_ne!(first, second);
        assert_eq!(first.parent(), Some(root));
    }

    #[test]
    fn a_missing_input_file_is_a_failure() {
        let (file, pages) = project("missing", "");
        fs::remove_file(&file).unwrap();
        let report = wait(&mut Job::start(&file, file.parent().unwrap(), pages.clone(), 1));
        assert!(!report.ok);
        assert!(!report.lines.is_empty());
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_failed_command_reports_its_stderr() {
        let mut command = Command::new("sh");
        command.args(["-c", "echo oops >&2; exit 3"]);
        let report = wait(&mut Job::spawn(command, PathBuf::new()));
        assert!(!report.ok);
        assert_eq!(report.lines, ["oops"]);
    }

    #[test]
    fn a_missing_program_is_a_failure() {
        let report = wait(&mut Job::spawn(Command::new("no-such-program-xyz"), PathBuf::new()));
        assert!(!report.ok);
        assert!(report.lines[0].contains("Cannot run no-such-program-xyz"), "{:?}", report.lines);
    }

    #[test]
    fn dropping_the_job_kills_and_reaps_the_process() {
        let mut command = Command::new("sleep");
        command.arg("30");
        let mut job = Job::spawn(command, PathBuf::new());
        let State::Running { child, .. } = &job.state else {
            panic!("the job did not start");
        };
        let proc_dir = PathBuf::from(format!("/proc/{}", child.id()));
        assert!(proc_dir.exists(), "the process is not running");
        assert!(job.try_report().is_none());

        drop(job);
        // The process is killed and reaped, so the kernel has removed its entry.
        assert!(!proc_dir.exists(), "the process is still in the process table");
    }
}
