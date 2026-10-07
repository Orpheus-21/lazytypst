//! `lazytypst --doctor`: one line for each thing that the program needs.

use std::{io, path::Path};

use ratatui_image::picker::ProtocolType;

/// The result of one check.
#[derive(Debug, PartialEq)]
pub struct Check {
    name: &'static str,
    ok: bool,
    text: String,
}

impl Check {
    fn new(name: &'static str, ok: bool, text: impl Into<String>) -> Self {
        Check {
            name,
            ok,
            text: text.into(),
        }
    }
}

/// The check of the Typst program. `version` is the result of `version_of` for that program.
pub fn typst_check(version: io::Result<String>) -> Check {
    match version {
        Ok(line) => Check::new("typst", true, line),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Check::new(
            "typst",
            false,
            "missing. Install it: https://github.com/typst/typst#installation",
        ),
        Err(err) => Check::new("typst", false, err.to_string()),
    }
}

/// The check of the terminal. `protocol` is what the image query found, or why it failed.
/// Half blocks always work, so they are not a failure.
pub fn terminal_check(protocol: Result<ProtocolType, String>) -> Check {
    let text = match protocol {
        Ok(ProtocolType::Kitty) => "kitty graphics".to_string(),
        Ok(ProtocolType::Sixel) => "sixel graphics".to_string(),
        Ok(ProtocolType::Iterm2) => "iTerm2 graphics".to_string(),
        Ok(ProtocolType::Halfblocks) => {
            "half blocks (the terminal reports no image protocol)".to_string()
        }
        Err(reason) => format!("half blocks (the query failed: {reason})"),
    };
    Check::new("terminal", true, text)
}

/// The check that `tmp` is writable: the program makes a file there and deletes it.
pub fn temp_check(tmp: &Path) -> Check {
    let probe = tmp.join(format!("lazytypst-doctor-{}", std::process::id()));
    // `create_new` fails if the name exists, also as a link. So the probe never writes through a link.
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            Check::new("temp folder", true, format!("{}, writable", tmp.display()))
        }
        Err(err) => Check::new(
            "temp folder",
            false,
            format!("{} is not writable: {err}", tmp.display()),
        ),
    }
}

/// The check for old `lazytypst-<pid>` folders of the user. A stale folder is not an error: the next
/// start deletes it. It is only a sign of an earlier crash, so the check passes.
pub fn stale_check(count: usize) -> Check {
    let text = match count {
        0 => "none".to_string(),
        1 => "1 (the next start deletes it)".to_string(),
        n => format!("{n} (the next start deletes them)"),
    };
    Check::new("stale folders", true, text)
}

/// The text of the report, and whether all checks passed.
pub fn report(lazytypst_version: &str, checks: &[Check]) -> (String, bool) {
    let mut text = format!("{:<14} {lazytypst_version}\n", "lazytypst");
    for check in checks {
        let status = if check.ok { "ok" } else { "fail" };
        text.push_str(&format!("{:<14} {status:<4} {}\n", check.name, check.text));
    }
    (text, checks.iter().all(|check| check.ok))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_working_typst_shows_its_version_and_a_missing_one_shows_the_install_link() {
        let ok = typst_check(Ok("typst 0.15.1 (9dfd3a08)".into()));
        assert!(ok.ok && ok.text == "typst 0.15.1 (9dfd3a08)");
        let missing = typst_check(Err(io::Error::from(io::ErrorKind::NotFound)));
        assert!(!missing.ok);
        assert!(missing.text.starts_with("missing"), "{}", missing.text);
        assert!(missing.text.contains("github.com/typst/typst"));
        let broken = typst_check(Err(io::Error::other("typst --version failed: 3")));
        assert!(!broken.ok && broken.text.contains("failed"));
    }

    #[test]
    fn the_terminal_line_names_the_protocol_and_half_blocks_are_not_a_failure() {
        assert_eq!(
            terminal_check(Ok(ProtocolType::Kitty)).text,
            "kitty graphics"
        );
        assert_eq!(
            terminal_check(Ok(ProtocolType::Sixel)).text,
            "sixel graphics"
        );
        let blocks = terminal_check(Err("not a terminal".into()));
        assert!(blocks.ok);
        assert!(blocks.text.contains("half blocks") && blocks.text.contains("not a terminal"));
    }

    #[test]
    fn the_temp_check_passes_for_a_writable_folder_and_fails_for_a_missing_one() {
        assert!(temp_check(&std::env::temp_dir()).ok);
        let missing = std::env::temp_dir().join("lazytypst-no-such-folder-for-the-doctor");
        let check = temp_check(&missing);
        assert!(
            !check.ok && check.text.contains("not writable"),
            "{}",
            check.text
        );
    }

    #[test]
    fn the_stale_check_counts_the_folders() {
        assert_eq!(stale_check(0).text, "none");
        assert!(stale_check(2).text.starts_with("2 "));
        assert!(stale_check(2).ok);
    }

    #[test]
    fn the_report_lines_up_and_the_result_is_false_if_one_check_fails() {
        let checks = [
            Check::new("typst", true, "typst 0.15.1"),
            Check::new("temp folder", false, "bad"),
        ];
        let (text, all_ok) = report("0.1.0", &checks);
        assert_eq!(
            text,
            "lazytypst      0.1.0\ntypst          ok   typst 0.15.1\ntemp folder    fail bad\n"
        );
        assert!(!all_ok);
        assert!(report("0.1.0", &checks[..1]).1);
    }
}
