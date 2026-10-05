use std::{
    fs, io,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc, Mutex,
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

/// A new folder inside `out_dir` for the pages of one editor.
/// A compile that outlives its editor then cannot write into the pages of the next editor.
pub fn new_out_dir() -> PathBuf {
    out_dir().join(NEXT_DIR.fetch_add(1, Ordering::Relaxed).to_string())
}

/// Deletes the page folder. The program calls this when it exits.
pub fn cleanup() {
    let _ = fs::remove_dir_all(out_dir());
}

/// A running command. Dropping the job kills the process, so a new job can replace an old one.
pub struct Job {
    /// `None` when the command did not start. Then the report is already waiting.
    child: Option<Arc<Mutex<Child>>>,
    receiver: Receiver<Report>,
}

impl Job {
    /// Runs `typst compile` on `file`. The pages go to `out_dir/page-{p}.png`.
    pub fn start(file: &Path, out_dir: &Path) -> Job {
        if let Err(err) = fs::create_dir_all(out_dir) {
            return Job::failed(format!("Cannot make {}: {err}", out_dir.display()));
        }
        let mut command = Command::new("typst");
        command
            .args(["compile", "--format", "png", "--diagnostic-format", "short"])
            .arg(file)
            .arg(out_dir.join("page-{p}.png"));
        Job::spawn(command)
    }

    fn failed(message: String) -> Job {
        let (tx, receiver) = mpsc::channel();
        let _ = tx.send(Report::failed(message));
        Job { child: None, receiver }
    }

    /// Starts the command. The lines it prints on stderr become the report.
    fn spawn(mut command: Command) -> Job {
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                let program = command.get_program().to_string_lossy().into_owned();
                return Job::failed(format!("Cannot run {program}: {err}"));
            }
        };
        let mut stderr = child.stderr.take().expect("stderr is piped");
        let child = Arc::new(Mutex::new(child));
        let waiter = Arc::clone(&child);
        let (tx, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            // The read ends when the process ends, or when `Drop` kills it.
            let _ = stderr.read_to_end(&mut bytes);
            let status = waiter.lock().unwrap().wait();
            // The receiver is gone when the job was dropped. Then nobody needs the report.
            let _ = tx.send(report(status, &bytes));
        });
        Job { child: Some(child), receiver }
    }

    /// Returns the report if the command has ended. Never waits.
    pub fn try_report(&self) -> Option<Report> {
        match self.receiver.try_recv() {
            Ok(report) => Some(report),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Report::failed("The compile thread stopped.")),
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if let Some(child) = &self.child {
            // The process may have ended already. Then `kill` returns an error that does not matter.
            let _ = child.lock().unwrap().kill();
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
    fn wait(job: &Job) -> Report {
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
        let report = wait(&Job::start(&file, &pages));
        assert!(report.ok, "{:?}", report.lines);
        assert!(report.lines.is_empty());
        let png = fs::read(pages.join("page-1.png")).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_error_report_names_the_line() {
        let (file, pages) = project("bad", "= Title\n#nope()\n");
        let report = wait(&Job::start(&file, &pages));
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
        let report = wait(&Job::start(&file, &pages));
        assert!(!report.ok);
        assert!(!report.lines.is_empty());
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_failed_command_reports_its_stderr() {
        let mut command = Command::new("sh");
        command.args(["-c", "echo oops >&2; exit 3"]);
        let report = wait(&Job::spawn(command));
        assert!(!report.ok);
        assert_eq!(report.lines, ["oops"]);
    }

    #[test]
    fn a_missing_program_is_a_failure() {
        let report = wait(&Job::spawn(Command::new("no-such-program-xyz")));
        assert!(!report.ok);
        assert!(report.lines[0].contains("Cannot run no-such-program-xyz"), "{:?}", report.lines);
    }

    #[test]
    fn dropping_the_job_kills_the_process() {
        let mut command = Command::new("sleep");
        command.arg("30");
        let job = Job::spawn(command);
        let child = Arc::clone(job.child.as_ref().unwrap());
        assert!(child.lock().unwrap().try_wait().unwrap().is_none(), "the process ended too early");

        drop(job);
        let start = Instant::now();
        loop {
            if let Some(status) = child.lock().unwrap().try_wait().unwrap() {
                assert!(!status.success());
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "the process is still running");
            thread::sleep(Duration::from_millis(10));
        }
    }
}
