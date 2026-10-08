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
    time::{Duration, Instant},
};

/// The result of one `typst compile` run.
pub struct Report {
    pub ok: bool,
    /// The lines that `typst` printed on stderr. Errors and warnings are here.
    pub lines: Vec<String>,
    /// One entry for each line of `lines`, parsed once when the report is made.
    pub diagnostics: Vec<Diagnostic>,
    /// How long the command ran. `None` if the command did not run.
    pub elapsed: Option<Duration>,
}

impl Report {
    pub fn new(ok: bool, lines: Vec<String>) -> Self {
        let diagnostics = lines.iter().map(|line| Diagnostic::parse(line)).collect();
        Self {
            ok,
            lines,
            diagnostics,
            elapsed: None,
        }
    }

    pub fn with_elapsed(mut self, elapsed: Duration) -> Self {
        self.elapsed = Some(elapsed);
        self
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self::new(false, vec![message.into()])
    }

    /// The number of lines that are errors with a position. See `Diagnostic::parse`.
    pub fn error_count(&self) -> usize {
        self.count(Severity::Error)
    }

    /// The number of lines that are warnings with a position. See `Diagnostic::parse`.
    pub fn warning_count(&self) -> usize {
        self.count(Severity::Warning)
    }

    fn count(&self, severity: Severity) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == severity)
            .count()
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
        let found = [
            (": error: ", Severity::Error),
            (": warning: ", Severity::Warning),
        ]
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
    #[cfg(test)]
    tests_clean_up::watch();
    out_dir_path()
}

fn out_dir_path() -> PathBuf {
    base_dir().join(format!("lazytypst-{}", std::process::id()))
}

/// The folder that holds the page folder of each run: `$XDG_RUNTIME_DIR`, or else the temporary directory.
/// `$XDG_RUNTIME_DIR` is made for this: it belongs to one user, only that user can open it, and the system
/// removes it at the last logout. The temporary directory is shared, so another user could take the name
/// `lazytypst-<process id>` first. A value that is not an absolute path to a folder of the user is ignored.
pub fn base_dir() -> PathBuf {
    use std::os::unix::fs::MetadataExt;
    base_dir_from(
        std::env::var_os("XDG_RUNTIME_DIR"),
        std::env::temp_dir(),
        fs::metadata("/proc/self").map(|own| own.uid()).ok(),
    )
}

fn base_dir_from(runtime: Option<std::ffi::OsString>, tmp: PathBuf, uid: Option<u32>) -> PathBuf {
    use std::os::unix::fs::MetadataExt;
    runtime
        .map(PathBuf::from)
        .filter(|dir| {
            dir.is_absolute()
                && fs::metadata(dir).is_ok_and(|meta| meta.is_dir() && Some(meta.uid()) == uid)
        })
        .unwrap_or(tmp)
}

/// The tests use the page folder of the program, and nothing else deletes it at the end of a test run. Each
/// test thread that asks for the folder holds a guard. When the last guard goes, the folder goes too. A test
/// that runs later makes it again.
#[cfg(test)]
mod tests_clean_up {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static ACTIVE: AtomicUsize = AtomicUsize::new(0);

    struct Guard;

    impl Guard {
        fn new() -> Self {
            ACTIVE.fetch_add(1, Ordering::SeqCst);
            Guard
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            if ACTIVE.fetch_sub(1, Ordering::SeqCst) == 1 {
                let _ = std::fs::remove_dir_all(super::out_dir_path());
            }
        }
    }

    pub fn watch() {
        thread_local! {
            static GUARD: Guard = Guard::new();
        }
        GUARD.with(|_| {});
    }
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
        // Earlier versions made their folder in the temporary directory, so look there too.
        for dir in scan_dirs() {
            remove_stale_dirs_in(&dir, own.uid());
        }
    }
}

/// The folders in which stale page folders can be: the base folder, and the temporary directory.
pub fn scan_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![base_dir()];
    let tmp = std::env::temp_dir();
    if !dirs.contains(&tmp) {
        dirs.push(tmp);
    }
    dirs
}

/// Deletes each stale folder in `tmp`. See `stale_dirs_in`.
fn remove_stale_dirs_in(tmp: &Path, uid: u32) {
    for dir in stale_dirs_in(tmp, uid) {
        let _ = fs::remove_dir_all(dir);
    }
}

/// The folders `lazytypst-<pid>` in `tmp` that are real folders (not symlinks), belong to the user `uid`,
/// and have no running process with that pid.
pub fn stale_dirs_in(tmp: &Path, uid: u32) -> Vec<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let Ok(entries) = fs::read_dir(tmp) else {
        return Vec::new();
    };
    let mut stale = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|name| name.strip_prefix("lazytypst-"))
        else {
            continue;
        };
        if pid.is_empty() || !pid.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if meta.is_dir() && meta.uid() == uid && !Path::new("/proc").join(pid).exists() {
            stale.push(entry.path());
        }
    }
    stale
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
        return Err(io::Error::other(format!(
            "{program} --version failed: {}",
            output.status
        )));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(String::from)
        .ok_or_else(|| io::Error::other(format!("{program} --version printed nothing")))
}

/// The program that runs Typst: the value of `LAZYTYPST_TYPST` if it is set and not empty, or else `typst`.
pub fn typst_program() -> String {
    program_from(std::env::var_os("LAZYTYPST_TYPST"))
}

fn program_from(value: Option<std::ffi::OsString>) -> String {
    value.filter(|value| !value.is_empty()).map_or_else(
        || "typst".into(),
        |value| value.to_string_lossy().into_owned(),
    )
}

/// How long a compile or an export may run. A document can take very long, or an endless time, and a
/// project from the internet is not trusted. Typst then stops, and the report says why.
const TIMEOUT: Duration = Duration::from_secs(60);
/// The most that Typst may allocate, in bytes of address space. A page of 500 cm by 500 cm at 432 pixels
/// per inch needs 29 GB, and that can stop the whole desktop.
const MEMORY_LIMIT: u64 = 8 << 30;
/// The most bytes of the error text that the program keeps. The program reads the rest and drops it.
const STDERR_LIMIT: usize = 1 << 20;

/// The command that runs `program`. If the `prlimit` program of util-linux is in `PATH`, it starts
/// `program` with a limit on the memory (`prlimit` runs `program` in its own process, so the job
/// that kills the process kills Typst too). Else `program` runs without a limit.
fn limited_command(program: &str) -> Command {
    match (find_in_path("prlimit"), find_in_path(program)) {
        (Some(prlimit), Some(program)) => {
            let mut command = Command::new(prlimit);
            command
                .arg(format!("--as={MEMORY_LIMIT}"))
                .arg("--")
                .arg(program);
            command
        }
        _ => Command::new(program),
    }
}

/// The path of the program `name`: `name` itself if it has a slash and it is a file, or else the first
/// file with that name in a folder of `PATH`.
fn find_in_path(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        return Path::new(name).is_file().then(|| PathBuf::from(name));
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

/// The version line of the Typst program. See `version_of` and `typst_program`.
pub fn typst_version() -> io::Result<String> {
    version_of(&typst_program())
}

static NEXT_EXPORT: AtomicUsize = AtomicUsize::new(0);

/// A running command. Dropping the job kills the process, so a new job can replace an old one.
pub struct Job {
    state: State,
    /// What the command writes: a folder of PNG pages, or one PDF file.
    output: PathBuf,
    /// For a PDF export: the temp file that Typst writes, and the PDF that replaces it at the end.
    /// Typst follows a symlink at the path that it writes. A project can hold a link such as
    /// `report.pdf` that points at another file of the user. So Typst writes a new file with a new name,
    /// and `rename` then replaces the link itself and never the file that the link points at.
    export: Option<(PathBuf, PathBuf)>,
}

enum State {
    /// The command did not start. The report waits here until `try_report` takes it.
    Failed(Option<Report>),
    /// The command runs. A thread reads its stderr and sends the bytes when the process closes stderr.
    Running {
        child: Child,
        stderr: Receiver<(Vec<u8>, Duration)>,
        started: Instant,
        timeout: Duration,
    },
}

impl Job {
    /// Runs `typst compile` on `file` for page number `page` only, counted from 1.
    /// The one file goes to `dir/page-<page>-of-<count>.png`, where `<count>` is the number of pages of the
    /// document. If the document has fewer than `page` pages, Typst exits with success and writes no file.
    /// Typst can read the files under `root`. So `file` can import files from parent folders inside `root`.
    /// Typst runs in `root`, so an error line names the file relative to `root`.
    /// `file` and `dir` must be absolute, or relative to `root`.
    #[cfg(test)]
    pub fn start(file: &Path, root: &Path, dir: PathBuf, page: usize) -> Job {
        Job::start_with(&typst_program(), file, root, dir, page, None)
    }

    /// `start` with a resolution in pixels per inch. `None` keeps the default of Typst, 144.
    pub fn start_ppi(file: &Path, root: &Path, dir: PathBuf, page: usize, ppi: Option<u32>) -> Job {
        Job::start_with(&typst_program(), file, root, dir, page, ppi)
    }

    /// `start_ppi` with the program that runs Typst.
    fn start_with(
        program: &str,
        file: &Path,
        root: &Path,
        dir: PathBuf,
        page: usize,
        ppi: Option<u32>,
    ) -> Job {
        if let Err(err) = fs::create_dir_all(&dir) {
            return Job::failed(format!("Cannot make {}: {err}", dir.display()), dir);
        }
        let mut command = limited_command(program);
        command
            .current_dir(root)
            .args([
                "compile",
                "--format",
                "png",
                "--diagnostic-format",
                "short",
                "--root",
            ])
            .arg(root)
            .arg("--pages")
            .arg(page.to_string());
        if let Some(ppi) = ppi {
            command.arg("--ppi").arg(ppi.to_string());
        }
        command.arg(file).arg(dir.join("page-{p}-of-{t}.png"));
        Job::spawn(command, dir)
    }

    /// Runs `typst compile` on `file`. The PDF goes to `pdf`. An old file at `pdf` is replaced.
    /// The paths follow the same rules as in `start`.
    pub fn start_pdf(file: &Path, root: &Path, pdf: PathBuf) -> Job {
        Job::start_pdf_with(&typst_program(), file, root, pdf)
    }

    /// `start_pdf` with the program that runs Typst.
    fn start_pdf_with(program: &str, file: &Path, root: &Path, pdf: PathBuf) -> Job {
        // Typst writes a new file with a new name. The end of the job renames it onto `pdf`. See `Job::export`.
        let tmp = pdf.with_file_name(format!(
            ".lazytypst-{}-{}.pdf.tmp",
            std::process::id(),
            NEXT_EXPORT.fetch_add(1, Ordering::Relaxed)
        ));
        if fs::symlink_metadata(&tmp).is_ok() {
            return Job::failed(format!("{} exists already", tmp.display()), pdf);
        }
        let mut command = limited_command(program);
        command
            .current_dir(root)
            .args([
                "compile",
                "--format",
                "pdf",
                "--diagnostic-format",
                "short",
                "--root",
            ])
            .arg(root)
            .arg(file)
            .arg(&tmp);
        let mut job = Job::spawn(command, pdf.clone());
        job.export = Some((tmp, pdf));
        job
    }

    /// The same as `start_pdf`, with another time limit. For a test.
    #[cfg(test)]
    fn start_pdf_timeout(
        program: &str,
        file: &Path,
        root: &Path,
        pdf: PathBuf,
        timeout: Duration,
    ) -> Job {
        let mut job = Job::start_pdf_with(program, file, root, pdf);
        if let State::Running { timeout: limit, .. } = &mut job.state {
            *limit = timeout;
        }
        job
    }

    #[cfg(test)]
    pub fn ended_with_success(output: PathBuf) -> Job {
        Job::spawn(Command::new("true"), output)
    }

    /// What the command writes: the page folder of `start`, or the PDF file of `start_pdf`.
    pub fn output(&self) -> &Path {
        &self.output
    }

    fn failed(message: String, output: PathBuf) -> Job {
        Job {
            state: State::Failed(Some(Report::failed(message))),
            output,
            export: None,
        }
    }

    /// Starts the command. The lines it prints on stderr become the report.
    fn spawn(mut command: Command, output: PathBuf) -> Job {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let started = Instant::now();
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
            // The thread keeps the first bytes and reads the rest to drop it, so a full pipe never stops Typst.
            let mut bytes = Vec::new();
            let mut chunk = [0u8; 8192];
            while let Ok(n @ 1..) = pipe.read(&mut chunk) {
                let keep = n.min(STDERR_LIMIT.saturating_sub(bytes.len()));
                bytes.extend_from_slice(&chunk[..keep]);
            }
            // The read ends when the process ends. So this is the time of the command, and not the time
            // until the main loop polls for the report.
            let elapsed = started.elapsed();
            // The receiver is gone when the job was dropped. Then nobody needs the bytes.
            let _ = tx.send((bytes, elapsed));
        });
        Job {
            state: State::Running {
                child,
                stderr,
                started,
                timeout: TIMEOUT,
            },
            output,
            export: None,
        }
    }

    /// Takes the report of the command if it has ended. Returns `None` while it runs.
    /// A command that runs longer than the time limit is killed, and the report says so.
    /// At the end of an export, the new PDF replaces the old file.
    pub fn try_report(&mut self) -> Option<Report> {
        let report = self.poll()?;
        let Some((tmp, pdf)) = self.export.take() else {
            return Some(report);
        };
        if !report.ok {
            let _ = fs::remove_file(&tmp);
            return Some(report);
        }
        // `rename` replaces a symlink at `pdf` and not the file that the link points at.
        match fs::rename(&tmp, &pdf) {
            Ok(()) => Some(report),
            Err(err) => {
                let _ = fs::remove_file(&tmp);
                Some(Report::failed(format!(
                    "Cannot write {}: {err}",
                    pdf.display()
                )))
            }
        }
    }

    fn poll(&mut self) -> Option<Report> {
        let (child, stderr, started, timeout) = match &mut self.state {
            State::Failed(report) => return report.take(),
            State::Running {
                child,
                stderr,
                started,
                timeout,
            } => (child, stderr, *started, *timeout),
        };
        let status = match child.try_wait() {
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Some(Report::failed(format!(
                    "Typst ran for more than {} seconds, so lazytypst stopped it.",
                    timeout.as_secs().max(1)
                )));
            }
            Ok(None) => return None,
            Ok(Some(status)) => status,
            Err(err) => return Some(report(Err(err), &[])),
        };
        match stderr.try_recv() {
            Ok((bytes, elapsed)) => Some(report(Ok(status), &bytes).with_elapsed(elapsed)),
            // The process has ended, but the thread has not sent the bytes yet. The next call gets them.
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(report(Ok(status), &[])),
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if let Some((tmp, _)) = &self.export {
            // The job ended before the rename, for example a new export replaced it.
            let _ = fs::remove_file(tmp);
        }
        if let State::Running { child, .. } = &mut self.state {
            // The process may have ended already. Then `kill` returns an error that does not matter.
            let _ = child.kill();
            // `wait` removes the dead process from the process table. After a kill it returns at once.
            let _ = child.wait();
        }
    }
}

fn report(status: io::Result<ExitStatus>, stderr: &[u8]) -> Report {
    let mut lines: Vec<String> = String::from_utf8_lossy(stderr)
        .lines()
        .map(String::from)
        .collect();
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

    /// Waits up to 10 seconds for the report of the job.
    fn wait(job: &mut Job) -> Report {
        let start = Instant::now();
        loop {
            if let Some(report) = job.try_report() {
                return report;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "the job did not end"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Makes a `doc.typ` with `content` in a new temporary folder. Returns the file and a page folder.
    fn project(name: &str, content: &str) -> (PathBuf, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("lazytypst-compile-{name}-{}", std::process::id()));
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
        Diagnostic {
            severity,
            file: Some(PathBuf::from(file)),
            line,
            column,
            message: message.into(),
        }
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
            at(
                "w.typ",
                1,
                16,
                Severity::Warning,
                "unknown font family: nosuchfont"
            )
        );
    }

    #[test]
    fn a_path_with_spaces_or_a_colon_is_parsed() {
        assert_eq!(
            parse("sp ace/a b.typ:1:1: error: unknown variable: nope"),
            at(
                "sp ace/a b.typ",
                1,
                1,
                Severity::Error,
                "unknown variable: nope"
            )
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
            at(
                "a.typ",
                2,
                3,
                Severity::Error,
                "expected: error: found: warning: x"
            )
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
        let report = Report::new(
            false,
            vec!["a.typ:1:2: error: boom".into(), "hint: x".into()],
        );
        assert_eq!(report.lines, ["a.typ:1:2: error: boom", "hint: x"]);
        assert_eq!(report.diagnostics.len(), 2);
        assert_eq!(report.diagnostics[0].severity, Severity::Error);
        assert_eq!(report.diagnostics[1].severity, Severity::Other);
        assert_eq!(
            Report::failed("Cannot run typst").diagnostics[0].severity,
            Severity::Other
        );
    }

    #[test]
    fn a_report_counts_its_errors_and_its_warnings() {
        let report = Report::new(
            false,
            vec![
                "a.typ:1:1: error: e1".into(),
                "hint: a hint".into(),
                "b.typ:2:2: warning: w1".into(),
                "c.typ:3:3: error: e2".into(),
                "error: no position".into(),
                "Cannot run typst".into(),
            ],
        );
        assert_eq!((report.error_count(), report.warning_count()), (2, 1));
        assert_eq!(
            (
                Report::new(true, vec![]).error_count(),
                Report::new(true, vec![]).warning_count()
            ),
            (0, 0)
        );
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
        assert_eq!(
            report.first_error(),
            Some(&at("b.typ", 4, 7, Severity::Error, "e1"))
        );
        assert_eq!(
            Report::new(true, vec!["a.typ:1:1: warning: w".into()]).first_error(),
            None
        );
        assert_eq!(Report::new(true, vec![]).first_error(), None);
    }

    #[test]
    fn a_real_typst_error_gets_a_position() {
        let (file, pages) = project("diag", "= Title\n#nope()\n");
        let report = wait(&mut Job::start(
            &file,
            file.parent().unwrap(),
            pages.clone(),
            1,
        ));
        let first = report.first_error().expect("an error with a position");
        assert_eq!((first.line, first.column), (2, 1), "{first:?}");
        assert!(first.message.contains("unknown variable"), "{first:?}");
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn typst_counts_the_column_in_characters_from_1() {
        for (text, column) in [
            ("é #nope()", 3),
            ("संस्कृतम् #nope()", 11),
            ("👨\u{200d}👩\u{200d}👧\u{200d}👦 #nope()", 9),
        ] {
            let (file, pages) = project("column", &format!("{text}\n"));
            let report = wait(&mut Job::start(
                &file,
                file.parent().unwrap(),
                pages.clone(),
                1,
            ));
            let first = report.first_error().expect("an error with a position");
            assert_eq!(first.column, column, "{text}");
            fs::remove_dir_all(file.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn a_valid_file_makes_a_png_page() {
        let (file, pages) = project("ok", "= Title\nSome text.\n");
        let report = wait(&mut Job::start(
            &file,
            file.parent().unwrap(),
            pages.clone(),
            1,
        ));
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
        assert!(
            report.lines[0].starts_with("chapters/c.typ:1:"),
            "{:?}",
            report.lines
        );
        let report = wait(&mut Job::start_pdf(&chapter, root, root.join("c.pdf")));
        assert!(
            report.lines[0].starts_with("chapters/c.typ:1:"),
            "{:?}",
            report.lines
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_compile_of_one_page_writes_exactly_one_png_named_with_the_page_count() {
        let (file, pages) = project(
            "onepage",
            "= One\n#pagebreak()\n= Two\n#pagebreak()\n= Three\n",
        );
        let report = wait(&mut Job::start(
            &file,
            file.parent().unwrap(),
            pages.clone(),
            2,
        ));
        assert!(report.ok, "{:?}", report.lines);
        let names: Vec<_> = fs::read_dir(&pages)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["page-2-of-3.png"]);
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_page_beyond_the_end_gives_success_and_no_file() {
        let (file, pages) = project("beyond", "= One\n#pagebreak()\n= Two\n");
        let report = wait(&mut Job::start(
            &file,
            file.parent().unwrap(),
            pages.clone(),
            5,
        ));
        assert!(report.ok, "{:?}", report.lines);
        assert_eq!(
            fs::read_dir(&pages).unwrap().count(),
            0,
            "typst wrote a file"
        );
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
        let report = wait(&mut Job::start_pdf(
            &file,
            file.parent().unwrap(),
            pdf.clone(),
        ));
        assert!(!report.ok);
        assert!(
            report.lines.iter().any(|l| l.contains(":1:")),
            "{:?}",
            report.lines
        );
        assert!(!pdf.exists());
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_error_report_names_the_line() {
        let (file, pages) = project("bad", "= Title\n#nope()\n");
        let report = wait(&mut Job::start(
            &file,
            file.parent().unwrap(),
            pages.clone(),
            1,
        ));
        assert!(!report.ok);
        assert!(
            report
                .lines
                .iter()
                .any(|l| l.contains(":2:") && l.contains("error")),
            "{:?}",
            report.lines
        );
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn the_runtime_folder_is_the_base_if_it_is_a_folder_of_the_user_and_else_the_temporary_directory()
     {
        let tmp = PathBuf::from("/tmp");
        let (dir, uid) = test_dir("basedir");
        let os = |path: &Path| Some(path.as_os_str().to_owned());
        assert_eq!(base_dir_from(os(&dir), tmp.clone(), Some(uid)), dir);
        // A value that is no good is ignored.
        assert_eq!(base_dir_from(None, tmp.clone(), Some(uid)), tmp);
        assert_eq!(
            base_dir_from(Some("relative/dir".into()), tmp.clone(), Some(uid)),
            tmp
        );
        assert_eq!(
            base_dir_from(os(&dir.join("missing")), tmp.clone(), Some(uid)),
            tmp
        );
        fs::write(dir.join("file"), "").unwrap();
        assert_eq!(
            base_dir_from(os(&dir.join("file")), tmp.clone(), Some(uid)),
            tmp
        );
        // A folder of another user is not trusted.
        assert_eq!(base_dir_from(os(&dir), tmp.clone(), Some(uid + 1)), tmp);
        assert_eq!(base_dir_from(os(&dir), tmp.clone(), None), tmp);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_page_folder_is_inside_the_base_folder() {
        assert_eq!(out_dir().parent(), Some(base_dir().as_path()));
        assert!(
            scan_dirs().contains(&std::env::temp_dir()),
            "folders of older versions are found"
        );
        assert!(
            out_dir()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("lazytypst-")
        );
    }

    /// A new empty folder for a test, and the user id of its owner.
    fn test_dir(name: &str) -> (PathBuf, u32) {
        let dir =
            std::env::temp_dir().join(format!("lazytypst-ownpid-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let uid = uid_of(&dir);
        (dir, uid)
    }

    #[test]
    fn a_fake_program_from_the_variable_runs_the_compile_and_the_export() {
        let (program, dir) = fake_program("lazytypst-typst-var", "echo \"fake: $*\" >&2; exit 3");
        let root = dir.clone();
        let file = root.join("a.typ");
        fs::write(&file, "").unwrap();
        let mut report = None;
        for _ in 0..50 {
            let mut job = Job::start_with(
                program.to_str().unwrap(),
                &file,
                &root,
                root.join("pages"),
                1,
                None,
            );
            let got = wait(&mut job);
            if got.lines.iter().any(|line| line.starts_with("Cannot run")) {
                thread::sleep(Duration::from_millis(20));
                continue;
            }
            report = Some(got);
            break;
        }
        let report = report.expect("the fake program must start");
        assert!(!report.ok);
        assert!(
            report.lines[0].starts_with("fake: compile --format png"),
            "{:?}",
            report.lines
        );
        let mut export =
            Job::start_pdf_with(program.to_str().unwrap(), &file, &root, root.join("a.pdf"));
        let report = wait(&mut export);
        assert!(
            report.lines[0].starts_with("fake: compile --format pdf"),
            "{:?}",
            report.lines
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_program_that_does_not_exist_shows_cannot_run_and_the_path() {
        let dir = std::env::temp_dir().join(format!("lazytypst-novar-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut job = Job::start_with(
            "/no/such/typst-0.14",
            &dir.join("a.typ"),
            &dir,
            dir.join("pages"),
            1,
            None,
        );
        let report = wait(&mut job);
        assert!(!report.ok);
        assert!(
            report.lines[0].starts_with("Cannot run /no/such/typst-0.14"),
            "{:?}",
            report.lines
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_variable_names_the_program_only_when_it_is_not_empty() {
        let os = |text: &str| Some(std::ffi::OsString::from(text));
        assert_eq!(program_from(None), "typst");
        assert_eq!(program_from(os("")), "typst");
        assert_eq!(program_from(os("/opt/typst-0.14")), "/opt/typst-0.14");
    }

    #[test]
    fn an_export_replaces_a_symlink_and_never_the_file_that_it_points_at() {
        let dir = std::env::temp_dir().join(format!("lazytypst-exportlink-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let victim = dir.join("victim.txt");
        fs::write(&victim, "precious").unwrap();
        fs::write(dir.join("a.typ"), "= Hi\n").unwrap();
        std::os::unix::fs::symlink(&victim, dir.join("a.pdf")).unwrap();
        // A dangling link must not make the target file either.
        let outside = dir.join("never-made.txt");
        fs::write(dir.join("b.typ"), "= Hi\n").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("b.pdf")).unwrap();

        for name in ["a", "b"] {
            let report = wait(&mut Job::start_pdf(
                &dir.join(format!("{name}.typ")),
                &dir,
                dir.join(format!("{name}.pdf")),
            ));
            assert!(report.ok, "{:?}", report.lines);
            let meta = fs::symlink_metadata(dir.join(format!("{name}.pdf"))).unwrap();
            assert!(
                meta.is_file(),
                "{name}.pdf must be a new file and not a link"
            );
            assert_eq!(
                fs::read(dir.join(format!("{name}.pdf"))).unwrap()[..4],
                *b"%PDF"
            );
        }
        assert_eq!(fs::read_to_string(&victim).unwrap(), "precious");
        assert!(!outside.exists());
        let stray: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().contains("lazytypst-"))
            .collect();
        assert!(stray.is_empty(), "no temp file stays: {stray:?}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_export_leaves_the_old_pdf_and_no_temp_file() {
        let dir = std::env::temp_dir().join(format!("lazytypst-exportfail-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.typ"), "#nope()\n").unwrap();
        fs::write(dir.join("a.pdf"), "old").unwrap();
        let report = wait(&mut Job::start_pdf(
            &dir.join("a.typ"),
            &dir,
            dir.join("a.pdf"),
        ));
        assert!(!report.ok);
        assert_eq!(fs::read_to_string(dir.join("a.pdf")).unwrap(), "old");
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_compile_that_runs_too_long_is_stopped_and_the_report_says_so() {
        let (program, dir) = fake_program("timeout", "sleep 30");
        let file = dir.join("a.typ");
        fs::write(&file, "").unwrap();
        let mut report = None;
        for _ in 0..50 {
            let mut job = Job::start_pdf_timeout(
                program.to_str().unwrap(),
                &file,
                &dir,
                dir.join("a.pdf"),
                Duration::from_millis(300),
            );
            let got = wait(&mut job);
            if got.lines.iter().any(|line| line.starts_with("Cannot run")) {
                thread::sleep(Duration::from_millis(20));
                continue;
            }
            report = Some(got);
            break;
        }
        let report = report.expect("the fake program must start");
        assert!(!report.ok);
        assert!(
            report.lines[0].contains("ran for more than"),
            "{:?}",
            report.lines
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_huge_error_text_is_cut_and_never_stops_the_command() {
        // 3 MB on stderr, in lines of 100 characters. The command must end, and the report is capped.
        let (program, dir) = fake_program(
            "bigerr",
            "head -c 3000000 /dev/zero | tr '\\0' 'x' | fold -w 100 >&2; exit 1",
        );
        let file = dir.join("a.typ");
        fs::write(&file, "").unwrap();
        let mut report = None;
        for _ in 0..50 {
            let mut job = Job::start_pdf_timeout(
                program.to_str().unwrap(),
                &file,
                &dir,
                dir.join("a.pdf"),
                Duration::from_secs(30),
            );
            let got = wait(&mut job);
            if got.lines.iter().any(|line| line.starts_with("Cannot run")) {
                thread::sleep(Duration::from_millis(20));
                continue;
            }
            report = Some(got);
            break;
        }
        let report = report.expect("the fake program must start");
        let total: usize = report.lines.iter().map(String::len).sum();
        assert!(total <= STDERR_LIMIT + 200, "{total}");
        assert!(total > 100_000, "the first part is kept: {total}");
        fs::remove_dir_all(dir).unwrap();
    }

    /// A program for a test: a shell script in a new folder. Returns the path of the script and its folder.
    fn fake_program(name: &str, script: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir =
            std::env::temp_dir().join(format!("lazytypst-fake-{name}-{}", std::process::id()));
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
        let (program, dir) = fake_program(
            "version",
            "echo 'typst 9.9.9 (abc123)'; echo 'a second line'",
        );
        assert_eq!(version_of_script(&program).unwrap(), "typst 9.9.9 (abc123)");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_program_gets_the_argument_version() {
        let (program, dir) = fake_program(
            "arg",
            r#"[ "$1" = "--version" ] && echo "got the argument" || echo "wrong: $*""#,
        );
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
    fn a_report_knows_how_long_the_command_ran() {
        let mut command = Command::new("sleep");
        command.arg("0.3");
        let report = wait(&mut Job::spawn(command, PathBuf::new()));
        let elapsed = report.elapsed.expect("a command that ran has a time");
        assert!(elapsed >= Duration::from_millis(300), "{elapsed:?}");
        assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
    }

    #[test]
    fn the_time_is_the_time_of_the_command_and_not_the_time_until_the_poll() {
        let job = &mut Job::ended_with_success(PathBuf::new());
        thread::sleep(Duration::from_millis(600)); // nobody polls for a while
        let elapsed = wait(job).elapsed.unwrap();
        assert!(
            elapsed < Duration::from_millis(400),
            "the time includes the wait for the poll: {elapsed:?}"
        );
    }

    #[test]
    fn a_report_of_a_command_that_did_not_start_has_no_time() {
        let report = wait(&mut Job::spawn(
            Command::new("no-such-program-xyz"),
            PathBuf::new(),
        ));
        assert_eq!(report.elapsed, None);
        assert_eq!(Report::failed("x").elapsed, None);
        assert_eq!(Report::new(true, vec![]).elapsed, None);
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
        assert_eq!(
            fs::read_dir(&path).unwrap().count(),
            0,
            "the folder must be new and empty"
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
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
        assert!(
            fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the link was removed"
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_dangling_symlink_at_the_path_stops_the_start_too() {
        let (base, uid) = test_dir("dangling");
        let path = base.join("lazytypst-123");
        std::os::unix::fs::symlink(base.join("nowhere"), &path).unwrap();
        assert_eq!(
            make_out_dir_for(&path, uid).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert!(!base.join("nowhere").exists(), "the link target was made");
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_file_at_the_path_stops_the_start_and_stays() {
        let (base, uid) = test_dir("file");
        let path = base.join("lazytypst-123");
        fs::write(&path, "keep").unwrap();
        assert_eq!(
            make_out_dir_for(&path, uid).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
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
        assert_eq!(
            make_out_dir_for(&path, uid + 1).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read_to_string(path.join("keep")).unwrap(), "keep");
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_private_dir_is_new_and_only_the_user_can_open_it() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("lazytypst-private-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        make_private_dir(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            make_private_dir(&path).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
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
        let tmp =
            std::env::temp_dir().join(format!("lazytypst-stale-{name}-{}", std::process::id()));
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
        assert!(
            fs::symlink_metadata(&link).is_ok(),
            "the symlink was deleted"
        );
        assert!(
            tmp.join("target").join("keep").exists(),
            "the symlink target was emptied"
        );
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
        let report = wait(&mut Job::start(
            &file,
            file.parent().unwrap(),
            pages.clone(),
            1,
        ));
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
        let report = wait(&mut Job::spawn(
            Command::new("no-such-program-xyz"),
            PathBuf::new(),
        ));
        assert!(!report.ok);
        assert!(
            report.lines[0].contains("Cannot run no-such-program-xyz"),
            "{:?}",
            report.lines
        );
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
        assert!(
            !proc_dir.exists(),
            "the process is still in the process table"
        );
    }
}
