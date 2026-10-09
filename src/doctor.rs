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

/// What the image query of the terminal gave.
#[derive(Debug)]
pub enum Query {
    /// The terminal reported this protocol (or none, as half blocks).
    Protocol(ProtocolType),
    /// The input or the output is not a terminal, so the program did not ask. A pipe or a file has no
    /// image protocol, and the query would go into the file.
    NotATerminal,
    /// The program asked, and the query failed.
    Failed(String),
}

/// The tmux hint: tmux blocks the image data unless `allow-passthrough` is on. It is shown only when
/// the program runs in tmux and the preview uses half blocks.
pub const TMUX_HINT: &str = "In tmux, set -g allow-passthrough on";

/// The check of the terminal. Half blocks always work, so they are not a failure. If the program could not
/// ask, the line says so and does not guess. `in_tmux` adds the tmux hint to the half blocks lines.
pub fn terminal_check(query: Query, in_tmux: bool) -> Check {
    let text = match query {
        Query::Protocol(ProtocolType::Kitty) => "kitty graphics".to_string(),
        Query::Protocol(ProtocolType::Sixel) => "sixel graphics".to_string(),
        Query::Protocol(ProtocolType::Iterm2) => "iTerm2 graphics".to_string(),
        Query::Protocol(ProtocolType::Halfblocks) => {
            "half blocks (the terminal reports no image protocol)".to_string()
        }
        Query::NotATerminal => {
            "unknown (the input or the output is not a terminal, so the program did not ask. Run it without a pipe or a redirect)".to_string()
        }
        Query::Failed(reason) => format!("half blocks (the query failed: {reason})"),
    };
    let hint = in_tmux && text.starts_with("half blocks");
    let text = if hint {
        format!("{text}. {TMUX_HINT}")
    } else {
        text
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
            terminal_check(Query::Protocol(ProtocolType::Kitty), false).text,
            "kitty graphics"
        );
        assert_eq!(
            terminal_check(Query::Protocol(ProtocolType::Sixel), false).text,
            "sixel graphics"
        );
        let blocks = terminal_check(Query::Failed("no answer".into()), false);
        assert!(blocks.ok);
        assert!(blocks.text.contains("half blocks") && blocks.text.contains("no answer"));
        let unknown = terminal_check(Query::NotATerminal, false);
        assert!(unknown.ok);
        assert!(unknown.text.starts_with("unknown"), "{}", unknown.text);
        assert!(!unknown.text.contains("half blocks"));
    }

    #[test]
    fn the_tmux_hint_shows_only_in_tmux_with_half_blocks() {
        let halfblocks = || Query::Protocol(ProtocolType::Halfblocks);
        assert!(terminal_check(halfblocks(), true).text.contains(TMUX_HINT));
        assert!(!terminal_check(halfblocks(), false).text.contains("tmux"));
        let kitty = terminal_check(Query::Protocol(ProtocolType::Kitty), true);
        assert!(!kitty.text.contains("tmux"));
        assert!(
            !terminal_check(Query::NotATerminal, true)
                .text
                .contains("tmux")
        );
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
