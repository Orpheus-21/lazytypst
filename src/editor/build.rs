use super::*;

impl Editor {
    pub(super) fn save_and_compile(&mut self) {
        if self.save(false) {
            self.start_compile();
        }
    }

    /// Starts a compile of the wanted page. The new compile replaces the running compile.
    pub(super) fn start_compile(&mut self) {
        self.compile_at = None;
        if self.watch_mode {
            self.start_watch(false);
            return;
        }
        self.stop_compile();
        let target = self.compile_target().to_path_buf();
        let dir = compile::next_dir(&self.pages_root);
        self.job = Some(Job::start_ppi(
            &target,
            &self.root,
            dir,
            self.preview.wanted_page(),
            self.preview.ppi(),
        ));
    }

    /// Starts the compile after the autosave wrote the file. With a running `typst watch` the write itself
    /// starts the compile, so the editor only waits for the result. Else it starts a compile.
    pub(super) fn compile_after_save(&mut self) {
        if self.watch_mode {
            self.start_watch(true);
        } else {
            self.start_compile();
        }
    }

    /// With the option `LAZYTYPST_WATCH=1`: makes sure that a `typst watch` runs for the wanted page, the
    /// target, the root, and the resolution, and waits for its next report. `after_save` is true when the file
    /// was just written: a watch that runs with the same settings compiles it by itself. Else the watch
    /// starts again (a page turn, `Ctrl-B`, and a change of the target all need it), and its first compile is
    /// as slow as a new `typst compile`.
    fn start_watch(&mut self, after_save: bool) {
        let params = compile::WatchParams {
            target: self.compile_target().to_path_buf(),
            root: self.root.clone(),
            page: self.preview.wanted_page(),
            ppi: self.preview.ppi(),
        };
        let reuse = after_save
            && self
                .watch
                .as_mut()
                .is_some_and(|watch| watch.alive() && watch.params == params);
        if !reuse {
            self.watch = None; // the old process ends
            let dir = self.pages_root.join("watch");
            let _ = fs::remove_dir_all(&dir);
            match compile::Watch::start(params, dir) {
                Ok(watch) => self.watch = Some(watch),
                Err(err) => {
                    self.watch_wait = None;
                    self.report = Some(Report::failed(format!("Cannot start typst watch: {err}")));
                    return;
                }
            }
        }
        self.watch_wait = Some(WatchWait {
            since: Instant::now(),
            compiling: false,
        });
    }

    /// True while a compile runs or the editor waits for the report of the watch.
    pub(super) fn compile_busy(&self) -> bool {
        self.job.is_some() || self.watch_wait.is_some()
    }

    /// The file that the compile and the export use: the main file, or else the open file.
    pub(super) fn compile_target(&self) -> &Path {
        self.main.as_deref().unwrap_or(&self.path)
    }

    /// Kills the running compile and deletes its page folder.
    pub(super) fn stop_compile(&mut self) {
        if self.watch.take().is_some() {
            let _ = fs::remove_dir_all(self.pages_root.join("watch"));
        }
        self.watch_wait = None;
        if let Some(job) = self.job.take() {
            let dir = job.output().to_path_buf();
            drop(job);
            let _ = fs::remove_dir_all(dir);
        }
    }

    /// Takes the report of a finished compile. Returns true when the screen must redraw.
    ///
    /// Typst exits with success and writes no page when the document has fewer pages than the wanted page,
    /// for example after the user deleted pages while the preview showed the last page. Then the editor
    /// recovers in two steps, so that the user lands on the last page:
    /// 1. It compiles page 1 and learns the page count from the file name.
    /// 2. It compiles the last page (see `after_load`).
    ///
    /// `recover` is set between the steps, so a second "no page" result is an error and cannot loop.
    pub(super) fn poll_compile(&mut self) -> bool {
        let Some((mut report, dir)) = (if self.watch_mode {
            self.poll_watch()
        } else {
            self.poll_job()
        }) else {
            return false;
        };
        if !report.ok {
            self.recover = None;
            let _ = fs::remove_dir_all(dir);
        } else if page_in(&dir).is_none()
            && self.recover.is_none()
            && self.preview.wanted_page() > 1
        {
            let _ = fs::remove_dir_all(dir);
            self.recover = Some(self.preview.wanted_page());
            self.preview.want(1);
            self.start_compile();
            return true; // the old report stays on screen until the recovery ends
        } else {
            match self.preview.load(dir) {
                Ok(()) => self.after_load(),
                Err(err) => {
                    self.recover = None;
                    report = Report::failed(err);
                }
            }
        }
        self.report = Some(report);
        true
    }

    /// The report of the finished `typst compile` and the folder of its page.
    fn poll_job(&mut self) -> Option<(Report, PathBuf)> {
        let job = self.job.as_mut()?;
        let report = job.try_report()?;
        let dir = job.output().to_path_buf();
        self.job = None;
        if let Some(paths) = compile::read_deps(&dir, &self.root) {
            self.deps = paths
                .into_iter()
                .map(|path| {
                    let time = disk_time(&path);
                    (path, time)
                })
                .collect();
        }
        Some((report, dir))
    }

    /// The report of the next compile of the running `typst watch`, and a new folder with its page. The
    /// page file moves out of the folder of the watch, so the next compile starts with an empty folder.
    /// A watch that ended, or that does not answer for a minute, gives a failed report. A watch that does
    /// not start a compile within `WATCH_PATIENCE` after a save (the save changed nothing that it
    /// watches) starts again.
    fn poll_watch(&mut self) -> Option<(Report, PathBuf)> {
        let mut restart = false;
        let watch = self.watch.as_mut()?;
        while let Some(event) = watch.try_event() {
            match event {
                compile::WatchEvent::Compiling => {
                    let wait = self.watch_wait.get_or_insert(WatchWait {
                        since: Instant::now(),
                        compiling: true,
                    });
                    wait.compiling = true;
                }
                compile::WatchEvent::Done(report) => {
                    self.watch_wait = None;
                    let fresh = compile::next_dir(&self.pages_root);
                    if fs::create_dir_all(&fresh).is_ok()
                        && let Some((number, count)) = page_in(&watch.dir)
                    {
                        let name = format!("page-{number}-of-{count}.png");
                        let _ = fs::rename(watch.dir.join(&name), fresh.join(&name));
                    }
                    // A page of an older compile with another page count must not stay.
                    if let Ok(entries) = fs::read_dir(&watch.dir) {
                        for entry in entries.flatten() {
                            let _ = fs::remove_file(entry.path());
                        }
                    }
                    let dir = watch.dir.clone();
                    if self.paused {
                        // A compile that Ctrl-B asked for while the live compile is off: the watch ends.
                        self.watch = None;
                        let _ = fs::remove_dir_all(dir);
                    }
                    return Some((report, fresh));
                }
            }
        }
        let waited = self
            .watch_wait
            .as_ref()
            .map(|wait| (wait.since.elapsed(), wait.compiling));
        if !watch.alive() {
            self.watch = None;
            self.watch_wait = None;
            return Some((
                Report::failed("typst watch stopped"),
                compile::next_dir(&self.pages_root),
            ));
        }
        match waited {
            Some((elapsed, _)) if elapsed > WATCH_TIMEOUT => {
                self.watch = None;
                self.watch_wait = None;
                return Some((
                    Report::failed("typst watch gave no report for a minute and was stopped"),
                    compile::next_dir(&self.pages_root),
                ));
            }
            Some((elapsed, false)) if elapsed > WATCH_PATIENCE => restart = true,
            _ => {}
        }
        if restart {
            self.start_watch(false);
        }
        None
    }

    /// The second step of the recovery: if the page that the user wanted is beyond the end of the
    /// document, ask for the last page.
    pub(super) fn after_load(&mut self) {
        let Some(wanted) = self.recover.take() else {
            return;
        };
        let count = self.preview.page_count();
        if wanted > count {
            self.preview.want(count);
            // If the page on screen is the last page already, there is nothing to render again.
            if self.preview.shown_page() != count {
                self.start_compile();
            }
        }
    }

    /// Takes the report of a finished export. The pane shows the path of the PDF, or the errors.
    pub(super) fn poll_export(&mut self) -> bool {
        let Some(job) = &mut self.export else {
            return false;
        };
        let Some(report) = job.try_report() else {
            return false;
        };
        let pdf = job.output().to_path_buf();
        self.export = None;
        if report.ok {
            let name = pdf.strip_prefix(&self.root).unwrap_or(&pdf);
            self.message = format!("Exported {}", name.display());
            self.exported = Some(pdf);
        } else {
            self.message = "The PDF export failed. The pane shows the errors.".into();
            self.report = Some(report);
        }
        true
    }
}
