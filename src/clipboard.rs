//! The system clipboard, through the OSC 52 escape sequence. The terminal sets the clipboard.
//! A terminal that does not support OSC 52 ignores the sequence, so nothing breaks.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// The escape sequence that puts `text` on the system clipboard.
pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, byte)| n | u32::from(*byte) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The program that reads the system clipboard: `wl-paste` on Wayland, `xclip` on X11.
#[derive(Clone, Debug, PartialEq)]
pub enum Tool {
    WlPaste(String),
    Xclip(String),
}

impl Tool {
    /// The tool for this session, or `None` if the session has no display or the program is missing.
    pub fn detect() -> Option<Self> {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty());
        let x11 = std::env::var_os("DISPLAY").is_some_and(|value| !value.is_empty());
        let candidates = [
            (wayland, Self::WlPaste("wl-paste".into())),
            (x11, Self::Xclip("xclip".into())),
        ];
        candidates
            .into_iter()
            .find(|(usable, tool)| *usable && tool.installed())
            .map(|(_, tool)| tool)
    }

    fn program(&self) -> &str {
        match self {
            Self::WlPaste(program) | Self::Xclip(program) => program,
        }
    }

    fn installed(&self) -> bool {
        let program = self.program();
        if program.contains('/') {
            return std::path::Path::new(program).is_file();
        }
        std::env::var_os("PATH")
            .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
    }

    /// The command that lists the types of the clipboard, and the command that reads one type.
    fn commands(&self, mime: Option<&str>) -> std::process::Command {
        let mut command = std::process::Command::new(self.program());
        match (self, mime) {
            (Self::WlPaste(_), None) => command.arg("--list-types"),
            (Self::WlPaste(_), Some(mime)) => command.args(["--no-newline", "--type", mime]),
            (Self::Xclip(_), None) => {
                command.args(["-selection", "clipboard", "-t", "TARGETS", "-o"])
            }
            (Self::Xclip(_), Some(mime)) => {
                command.args(["-selection", "clipboard", "-t", mime, "-o"])
            }
        };
        command
    }
}

/// The most bytes of an image from the clipboard.
pub const MAX_IMAGE: usize = 20 << 20;
/// How long the program waits for the tool.
const TOOL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The image types that Typst reads, with the ending of the file for each. The first one that the
/// clipboard has is used.
const IMAGE_TYPES: [(&str, &str); 5] = [
    ("image/png", "png"),
    ("image/jpeg", "jpg"),
    ("image/webp", "webp"),
    ("image/gif", "gif"),
    ("image/svg+xml", "svg"),
];

/// Runs the command, and returns at most `limit` bytes of its output. A command that runs longer than
/// `TOOL_TIMEOUT`, or that fails, gives an error.
fn run(mut command: std::process::Command, limit: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|err| format!("cannot start the clipboard program: {err}"))?;
    let mut out = child.stdout.take().expect("stdout is piped");
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = out.by_ref().take(limit as u64 + 1).read_to_end(&mut bytes);
        bytes
    });
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < TOOL_TIMEOUT => {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("the clipboard program did not answer".into());
            }
        }
    };
    let bytes = reader.join().unwrap_or_default();
    if !status.success() {
        return Err("the clipboard has no content".into());
    }
    if bytes.len() > limit {
        return Err(format!("the image is larger than {} MiB", limit >> 20));
    }
    Ok(bytes)
}

/// The image on the clipboard, and the ending of its file. The error is a sentence for the status line.
pub fn read_image(tool: &Tool) -> Result<(Vec<u8>, &'static str), String> {
    let types = run(tool.commands(None), 64 << 10).map_err(|err| format!("No image: {err}."))?;
    let types = String::from_utf8_lossy(&types);
    let Some((mime, ending)) = IMAGE_TYPES
        .iter()
        .find(|(mime, _)| types.lines().any(|line| line.trim() == *mime))
    else {
        return Err(
            "The clipboard has no image that Typst reads (png, jpg, webp, gif, svg).".into(),
        );
    };
    let bytes =
        run(tool.commands(Some(mime)), MAX_IMAGE).map_err(|err| format!("No image: {err}."))?;
    if bytes.is_empty() {
        return Err("The clipboard image is empty.".into());
    }
    Ok((bytes, ending))
}

/// The name of a pasted image: `pasted-YYYYMMDD-HHMMSS.<ending>`, with the time in UTC.
pub fn pasted_name(now: std::time::SystemTime, ending: &str) -> String {
    let seconds = now
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.as_secs());
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    // The calendar date of the day number (days from 1970-01-01), by the method of Howard Hinnant.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "pasted-{year:04}{month:02}{day:02}-{:02}{:02}{:02}.{ending}",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_of_a_pasted_image_has_the_date_and_the_time_in_utc() {
        use std::time::{Duration, UNIX_EPOCH};
        let at = |seconds| UNIX_EPOCH + Duration::from_secs(seconds);
        assert_eq!(pasted_name(at(0), "png"), "pasted-19700101-000000.png");
        assert_eq!(
            pasted_name(at(951_782_400), "png"),
            "pasted-20000229-000000.png",
            "a leap day"
        );
        assert_eq!(
            pasted_name(at(951_782_400 + 86_399), "jpg"),
            "pasted-20000229-235959.jpg"
        );
        assert_eq!(
            pasted_name(at(951_868_800), "png"),
            "pasted-20000301-000000.png",
            "the day after a leap day"
        );
        assert_eq!(
            pasted_name(at(1_791_552_663), "png"),
            "pasted-20261009-133103.png"
        );
    }

    #[test]
    fn base64_matches_the_known_examples() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn the_sequence_holds_text_with_spaces_and_non_ascii_characters() {
        // "a b é 中" in UTF-8
        assert_eq!(osc52("a b é 中"), "\x1b]52;c;YSBiIMOpIOS4rQ==\x07");
    }
}
