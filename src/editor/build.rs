use super::*;

impl Editor {
    pub(super) fn save_and_compile(&mut self) {
        if self.save(false) {
            self.start_compile();
        }
    }

    /// Starts a compile of the wanted page. The new compile replaces the running compile.
    pub(super) fn start_compile(&mut self) {
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

    /// The file that the compile and the export use: the main file, or else the open file.
    pub(super) fn compile_target(&self) -> &Path {
        self.main.as_deref().unwrap_or(&self.path)
    }

    /// Kills the running compile and deletes its page folder.
    pub(super) fn stop_compile(&mut self) {
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
        let Some(job) = &mut self.job else {
            return false;
        };
        let Some(mut report) = job.try_report() else {
            return false;
        };
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
