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

/// A new folder name inside `root` for the pages of one compile.
/// No two compiles share a folder. So a compile that was killed cannot add pages to a newer one.
pub fn next_dir(root: &Path) -> PathBuf {
    root.join(NEXT_DIR.fetch_add(1, Ordering::Relaxed).to_string())
}

/// Makes the page folder of this run. Only the user can open it.
/// The call fails if the path exists already, also as a symlink. So the program never writes
/// into a folder that another user prepared, and the cleanup never deletes files of another user.
pub fn make_out_dir() -> io::Result<()> {
    make_private_dir(&out_dir())
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
    /// Runs `typst compile` on `file`. The pages go to `dir/page-{p}.png`.
    /// Typst can read the files under `root`. So `file` can import files from parent folders inside `root`.
    pub fn start(file: &Path, root: &Path, dir: PathBuf) -> Job {
        if let Err(err) = fs::create_dir_all(&dir) {
            return Job::failed(format!("Cannot make {}: {err}", dir.display()), dir);
        }
        let mut command = Command::new("typst");
        command
            .args(["compile", "--format", "png", "--diagnostic-format", "short", "--root"])
            .arg(root)
            .arg(file)
            .arg(dir.join("page-{p}.png"));
        Job::spawn(command, dir)
    }

    /// Runs `typst compile` on `file`. The PDF goes to `pdf`. An old file at `pdf` is replaced.
    pub fn start_pdf(file: &Path, root: &Path, pdf: PathBuf) -> Job {
        let mut command = Command::new("typst");
        command
            .args(["compile", "--format", "pdf", "--diagnostic-format", "short", "--root"])
            .arg(root)
            .arg(file)
            .arg(&pdf);
        Job::spawn(command, pdf)
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
            Report { ok, lines }
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

    #[test]
    fn a_valid_file_makes_a_png_page() {
        let (file, pages) = project("ok", "= Title\nSome text.\n");
        let report = wait(&mut Job::start(&file, file.parent().unwrap(), pages.clone()));
        assert!(report.ok, "{:?}", report.lines);
        assert!(report.lines.is_empty());
        let png = fs::read(pages.join("page-1.png")).unwrap();
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

        let report = wait(&mut Job::start(&chapter, root, pages.clone()));
        assert!(report.ok, "{:?}", report.lines);
        let report = wait(&mut Job::start_pdf(&chapter, root, root.join("c.pdf")));
        assert!(report.ok, "{:?}", report.lines);
        fs::remove_dir_all(root).unwrap();
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
        let report = wait(&mut Job::start(&file, file.parent().unwrap(), pages.clone()));
        assert!(!report.ok);
        assert!(
            report.lines.iter().any(|l| l.contains(":2:") && l.contains("error")),
            "{:?}",
            report.lines
        );
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
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
        let report = wait(&mut Job::start(&file, file.parent().unwrap(), pages.clone()));
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
