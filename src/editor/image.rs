use std::io::Write;

use super::*;
use crate::clipboard;

/// The folder for pasted images, inside the project: the variable `LAZYTYPST_IMAGES`, or else `images`.
fn images_folder() -> String {
    std::env::var("LAZYTYPST_IMAGES")
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "images".into())
}

/// Makes the folder `name` (a relative path) inside `root`, one level after the other. A level that is a
/// link or a file is refused: a link could lead out of the project.
pub(super) fn make_folder(root: &Path, name: &str) -> Result<PathBuf, String> {
    let relative = Path::new(name);
    let plain = relative
        .components()
        .all(|part| matches!(part, std::path::Component::Normal(_)))
        && relative.components().count() <= 4;
    if !plain || relative.as_os_str().is_empty() {
        return Err(format!(
            "The folder {name:?} must be a short path inside the project."
        ));
    }
    let mut folder = root.to_path_buf();
    for part in relative.components() {
        folder.push(part);
        match fs::symlink_metadata(&folder) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return Err(format!(
                    "{} is a link or a file, not a folder.",
                    folder.display()
                ));
            }
            Err(_) => fs::create_dir(&folder)
                .map_err(|err| format!("Cannot make {}: {err}", folder.display()))?,
        }
    }
    Ok(relative.to_path_buf())
}

impl Editor {
    /// `Alt-I`: saves the image on the system clipboard in the project, in the folder for images, and puts
    /// `#image("/images/pasted-....png", width: 80%)` at the cursor. The path starts with `/`, so it
    /// is right in a file of any folder. One edit, so one undo takes the line back (the file stays).
    pub(super) fn paste_image(&mut self) {
        let Some(tool) = self.clip_tool.clone() else {
            self.message =
                "No clipboard program. Install wl-clipboard (Wayland) or xclip (X11).".into();
            return;
        };
        let (bytes, ending) = match clipboard::read_image(&tool) {
            Ok(image) => image,
            Err(message) => {
                self.message = message;
                return;
            }
        };
        let folder = match make_folder(&self.root, &images_folder()) {
            Ok(folder) => folder,
            Err(message) => {
                self.message = message;
                return;
            }
        };
        let name = clipboard::pasted_name(SystemTime::now(), ending);
        let (stem, _) = name.rsplit_once('.').unwrap_or((&name, ""));
        // A second image in the same second gets a number. `create_new` never replaces a file.
        let mut saved = None;
        for attempt in 1..100 {
            let file = if attempt == 1 {
                name.clone()
            } else {
                format!("{stem}-{attempt}.{ending}")
            };
            let path = self.root.join(&folder).join(&file);
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut handle) => {
                    if let Err(err) = handle.write_all(&bytes) {
                        let _ = fs::remove_file(&path);
                        self.message = format!("Cannot write {}: {err}", path.display());
                        return;
                    }
                    saved = Some(file);
                    break;
                }
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(err) => {
                    self.message = format!("Cannot write {}: {err}", path.display());
                    return;
                }
            }
        }
        let Some(file) = saved else {
            self.message = "No free name for the image.".into();
            return;
        };
        let reference = format!("{}/{file}", folder.display());
        self.textarea.cancel_selection();
        self.textarea
            .insert_str(format!("#image(\"/{reference}\", width: 80%)"));
        self.mark_edit();
        self.message = format!("Saved {reference}");
    }
}
