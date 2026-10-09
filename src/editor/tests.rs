use super::*;
use ratatui::{Terminal, backend::TestBackend};

/// Opens the file. The pages of each compile go to the folder `pages` next to the file,
/// so no test writes into the temporary folder that the whole program shares.
fn open(path: &std::path::Path) -> Editor {
    open_with_main(path, None)
}

/// Opens the file with `main` as the main file. The root is the nearest folder above the file
/// that holds `main`. Without a main file, the root is the folder of the file.
fn open_with_main(path: &std::path::Path, main: Option<&str>) -> Editor {
    let root = match main {
        Some(name) => path
            .ancestors()
            .skip(1)
            .find(|dir| dir.join(name).exists())
            .unwrap()
            .to_path_buf(),
        None => path.parent().unwrap().to_path_buf(),
    };
    let main = main.map(|name| root.join(name));
    let mut editor =
        Editor::open(path.to_path_buf(), root.clone(), main, Picker::halfblocks()).unwrap();
    editor.pages_root = root.join("pages");
    editor
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// Makes a file with `content` in a new temporary folder.
fn temp_file(name: &str, content: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lazytypst-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("doc.typ");
    fs::write(&path, content).unwrap();
    path
}

/// Waits up to 20 seconds for the running compile to finish.
fn wait_for_report(editor: &mut Editor) {
    let start = Instant::now();
    while !editor.poll_compile() {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "compile did not finish"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn ctrl_s_writes_the_buffer_to_disk() {
    let path = temp_file("save", "hello\nworld\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    assert!(editor.dirty);
    assert_eq!(fs::read_to_string(&path).unwrap(), "hello\nworld\n");

    editor.handle_key(ctrl('s'));
    assert!(!editor.dirty);
    assert_eq!(fs::read_to_string(&path).unwrap(), "Xhello\nworld\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_save_writes_through_a_symlink_and_keeps_it() {
    let real = temp_file("symlink", "text\n");
    let link = real.with_file_name("link.typ");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let mut editor = open(&link);
    editor.handle_key(key(KeyCode::Char('X')));
    editor.handle_key(ctrl('s'));
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(&real).unwrap(), "Xtext\n");
    fs::remove_dir_all(real.parent().unwrap()).unwrap();
}

#[test]
fn a_save_keeps_the_permissions_and_leaves_no_temp_file() {
    use std::os::unix::fs::PermissionsExt;
    let path = temp_file("perms", "text\n");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    editor.handle_key(ctrl('s'));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    let names: Vec<_> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["doc.typ"], "a temp file is left behind");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_save_never_writes_through_a_planted_symlink() {
    // A cloned repository can contain a symlink at the old, fixed temp path.
    let path = temp_file("planted", "= Hi\n");
    let dir = path.parent().unwrap();
    let victim = dir.join("victim.txt");
    fs::write(&victim, "IMPORTANT\n").unwrap();
    std::os::unix::fs::symlink(&victim, dir.join(".doc.typ.lazytypst-tmp")).unwrap();

    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    editor.handle_key(ctrl('s'));
    assert_eq!(
        fs::read_to_string(&victim).unwrap(),
        "IMPORTANT\n",
        "the save wrote through the link"
    );
    assert!(
        !fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink(),
        "doc.typ became a link"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "X= Hi\n");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_new_file_is_not_created_through_a_dangling_symlink() {
    let path = temp_file("dangling", "text\n");
    let dir = path.parent().unwrap();
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(dir.join("elsewhere.txt"), &path).unwrap();

    editor.handle_key(ctrl('s'));
    assert!(
        !dir.join("elsewhere.txt").exists(),
        "the save created the link target"
    );
    assert!(editor.message.contains("Save failed"), "{}", editor.message);
    fs::remove_dir_all(dir).unwrap();
}

/// Makes `main.typ`, which includes `chapters/one.typ` with `chapter`. Returns the path of the chapter.
fn book(name: &str, chapter: &str) -> PathBuf {
    let main = temp_file(name, "#include \"chapters/one.typ\"\n");
    let dir = main.parent().unwrap().to_path_buf();
    fs::rename(&main, dir.join("main.typ")).unwrap();
    fs::create_dir_all(dir.join("chapters")).unwrap();
    fs::write(dir.join("chapters").join("one.typ"), chapter).unwrap();
    dir.join("chapters").join("one.typ")
}

#[test]
fn with_a_main_file_ctrl_e_exports_the_main_file() {
    let chapter = book("main-export", "= One\n");
    let dir = chapter.parent().unwrap().parent().unwrap().to_path_buf();
    let mut editor = open_with_main(&chapter, Some("main.typ"));
    editor.handle_key(ctrl('e'));
    wait_for_export(&mut editor);

    assert!(dir.join("main.pdf").exists(), "main.pdf is missing");
    assert!(
        !dir.join("chapters").join("one.pdf").exists(),
        "the chapter was exported"
    );
    assert!(editor.exported.as_ref().unwrap().ends_with("main.pdf"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn with_a_main_file_ctrl_b_compiles_the_main_file_and_names_the_chapter_in_errors() {
    let chapter = book("main-error", "#nope()\n");
    let dir = chapter.parent().unwrap().parent().unwrap().to_path_buf();
    let mut editor = open_with_main(&chapter, Some("main.typ"));
    // The buffer is clean. Ctrl-B must still compile.
    editor.handle_key(ctrl('b'));
    assert!(editor.job.is_some());
    wait_for_report(&mut editor);

    let report = editor.report.as_ref().unwrap();
    assert!(!report.ok);
    assert!(
        report
            .lines
            .iter()
            .any(|l| l.starts_with("chapters/one.typ:1:")),
        "{:?}",
        report.lines
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn with_a_main_file_the_autosave_of_a_chapter_updates_the_preview_of_the_main_file() {
    let chapter = book("main-live", "= One\n");
    let dir = chapter.parent().unwrap().parent().unwrap().to_path_buf();
    let mut editor = open_with_main(&chapter, Some("main.typ"));
    editor.handle_key(key(KeyCode::Char('X')));
    assert!(editor.tick(Instant::now() + Duration::from_millis(400)));

    assert_eq!(
        fs::read_to_string(&chapter).unwrap(),
        "X= One\n",
        "the chapter must be saved"
    );
    assert_eq!(
        fs::read_to_string(dir.join("main.typ")).unwrap(),
        "#include \"chapters/one.typ\"\n"
    );
    wait_for_report(&mut editor);
    assert!(editor.report.as_ref().unwrap().ok);
    assert!(editor.preview.has_page());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_title_names_the_main_file_when_it_is_not_the_open_file() {
    let chapter = book("main-title", "= One\n");
    let dir = chapter.parent().unwrap().parent().unwrap().to_path_buf();
    let mut editor = open_with_main(&chapter, Some("main.typ"));
    let text = screen_text(&mut editor);
    assert!(text.contains("chapters/one.typ (main: main.typ)"), "{text}");

    let mut editor = open_with_main(&dir.join("main.typ"), Some("main.typ"));
    let text = screen_text(&mut editor);
    assert!(
        text.contains("main.typ") && !text.contains("(main:"),
        "{text}"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn without_a_main_file_the_open_file_is_the_compile_target() {
    let chapter = book("main-none", "= One\n");
    let dir = chapter.parent().unwrap().parent().unwrap().to_path_buf();
    let mut editor = open(&chapter);
    editor.handle_key(ctrl('e'));
    wait_for_export(&mut editor);
    assert!(dir.join("chapters").join("one.pdf").exists());
    assert!(!dir.join("main.pdf").exists());
    fs::remove_dir_all(dir).unwrap();
}

/// Compiles the file in the editor and waits for the report.
fn compile_and_wait(editor: &mut Editor) {
    editor.handle_key(ctrl('b'));
    wait_for_report(editor);
}

#[test]
fn ctrl_g_moves_the_cursor_to_the_first_error() {
    let path = temp_file("goto", "= Title\n#nope()\n");
    let mut editor = open(&path);
    compile_and_wait(&mut editor);
    editor.handle_key(ctrl('g'));
    assert_eq!(editor.textarea.cursor(), (1, 0));
    assert!(editor.message.contains("2:1"), "{}", editor.message);
    assert!(!editor.dirty, "Ctrl-G must not change the text");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn ctrl_g_uses_the_column_of_the_error() {
    let path = temp_file("gotocolumn", "ab #nope()\n");
    let mut editor = open(&path);
    compile_and_wait(&mut editor);
    editor.handle_key(ctrl('g'));
    assert_eq!(
        editor.textarea.cursor(),
        (0, 3),
        "the cursor must stand on the #"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn ctrl_g_counts_the_column_in_characters_not_bytes() {
    for text in [
        "é #nope()",
        "संस्कृतम् #nope()",
        "👨\u{200d}👩\u{200d}👧\u{200d}👦 #nope()",
    ] {
        let path = temp_file("gotounicode", &format!("{text}\n"));
        let mut editor = open(&path);
        compile_and_wait(&mut editor);
        editor.handle_key(ctrl('g'));
        let cursor = editor.textarea.cursor();
        let (row, column) = (cursor.0, cursor.1);
        assert_eq!(
            editor.textarea.lines()[row].chars().nth(column),
            Some('#'),
            "{text}"
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

#[test]
fn ctrl_g_with_an_error_in_another_file_of_the_project_asks_to_open_it_after_a_save() {
    let chapter = book("gotoother", "= One\n");
    let dir = chapter.parent().unwrap().parent().unwrap().to_path_buf();
    fs::write(
        dir.join("main.typ"),
        "#nope()\n#include \"chapters/one.typ\"\n",
    )
    .unwrap();
    let mut editor = open_with_main(&chapter, Some("main.typ"));
    editor.handle_key(key(KeyCode::Right));
    editor.handle_key(key(KeyCode::Right));
    compile_and_wait(&mut editor);

    editor.handle_key(key(KeyCode::Char('X')));
    let action = editor.handle_key(ctrl('g'));
    assert!(
        matches!(&action, Action::Goto { file, line: 1, column: 1 } if file == Path::new("main.typ")),
        "the error is in main.typ at 1:1"
    );
    assert_eq!(
        fs::read_to_string(chapter).unwrap(),
        "= XOne\n",
        "the text is saved before the switch"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn ctrl_g_with_an_error_in_a_package_file_names_the_file_and_opens_nothing() {
    let path = temp_file("gotopackage", "text\n");
    let mut editor = open(&path);
    editor.report = report_with(false, &["@preview/cetz:0.2.0/src/lib.typ:5:3: error: boom"]);
    let action = editor.handle_key(ctrl('g'));
    assert!(matches!(action, Action::Stay));
    assert!(
        editor.message.contains("@preview/cetz:0.2.0/src/lib.typ"),
        "{}",
        editor.message
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn ctrl_g_with_an_error_in_a_file_outside_the_project_does_not_open_it() {
    let path = temp_file("gotooutside", "text\n");
    let mut editor = open(&path);
    editor.report = report_with(false, &["../outside.typ:1:1: error: boom"]);
    assert!(matches!(editor.handle_key(ctrl('g')), Action::Stay));
    assert!(editor.message.contains("outside.typ"), "{}", editor.message);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn ctrl_g_without_an_error_says_so_and_keeps_the_cursor() {
    let path = temp_file("gotonone", "= Title\ntext\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Down));
    editor.handle_key(ctrl('g')); // before any compile
    assert_eq!(editor.textarea.cursor(), (1, 0));
    assert!(editor.message.contains("No error"), "{}", editor.message);

    compile_and_wait(&mut editor);
    assert!(editor.report.as_ref().unwrap().ok);
    editor.handle_key(ctrl('g'));
    assert_eq!(editor.textarea.cursor(), (1, 0));
    assert!(editor.message.contains("No error"), "{}", editor.message);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn ctrl_g_ignores_a_warning() {
    let path = temp_file("gotowarning", "#set text(font: \"NoSuchFont\")\nHello\n");
    let mut editor = open(&path);
    compile_and_wait(&mut editor);
    assert!(
        editor
            .report
            .as_ref()
            .unwrap()
            .lines
            .iter()
            .any(|l| l.contains("warning"))
    );
    editor.handle_key(ctrl('g'));
    assert_eq!(editor.textarea.cursor(), (0, 0));
    assert!(editor.message.contains("No error"), "{}", editor.message);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn ctrl_g_with_an_old_report_stays_inside_the_text() {
    let path = temp_file("gotostale", "one\ntwo\n");
    let mut editor = open(&path);
    editor.report = Some(Report::new(false, vec!["doc.typ:50:90: error: x".into()]));
    editor.handle_key(ctrl('g'));
    assert_eq!(
        editor.textarea.cursor(),
        (1, 3),
        "the cursor goes to the end of the last line"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn ctrl_s_recreates_a_file_that_another_program_deleted() {
    let path = temp_file("deleted", "text\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    fs::remove_file(&path).unwrap();
    editor.handle_key(ctrl('s'));
    assert_eq!(fs::read_to_string(&path).unwrap(), "Xtext\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn an_unchanged_file_keeps_its_bytes() {
    let path = temp_file("same", "a\n\nb\n");
    let mut editor = open(&path);
    editor.handle_key(ctrl('s'));
    assert_eq!(fs::read_to_string(&path).unwrap(), "a\n\nb\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn an_empty_file_stays_empty() {
    let path = temp_file("empty", "");
    let mut editor = open(&path);
    editor.handle_key(ctrl('s'));
    assert_eq!(fs::read_to_string(&path).unwrap(), "");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn esc_closes_a_clean_buffer_at_once() {
    let path = temp_file("clean", "text\n");
    let mut editor = open(&path);
    assert!(matches!(
        editor.handle_key(key(KeyCode::Esc)),
        Action::Close
    ));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn esc_saves_the_text_then_closes() {
    let path = temp_file("escsave", "text\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    assert!(matches!(
        editor.handle_key(key(KeyCode::Esc)),
        Action::Close
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), "Xtext\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// Writes the file and gives it a modification time that differs from the old one.
fn change_outside(path: &Path, text: &str, seconds: u64) {
    fs::write(path, text).unwrap();
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(seconds))
        .unwrap();
}

#[test]
fn an_outside_change_loads_without_edits_and_keeps_the_line() {
    let path = temp_file("watch", "= One\nline two\nline three\n");
    let mut editor = open(&path);
    editor.textarea.move_cursor(CursorMove::Jump(2, 3));
    change_outside(&path, "= New\nb\nc\nd\n", 5);

    assert!(editor.tick(Instant::now() + Duration::from_secs(2)));
    assert_eq!(editor.textarea.lines(), ["= New", "b", "c", "d"]);
    assert_eq!(
        editor.textarea.cursor(),
        (2, 1),
        "the line stays, the column fits"
    );
    assert!(editor.job.is_some(), "the change starts a compile");
    assert!(!editor.dirty);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_shorter_file_puts_the_cursor_on_the_last_line() {
    let path = temp_file("watchshort", "a\nb\nc\n");
    let mut editor = open(&path);
    editor.textarea.move_cursor(CursorMove::Jump(2, 0));
    change_outside(&path, "x\n", 5);

    editor.tick(Instant::now() + Duration::from_secs(2));
    assert_eq!(editor.textarea.lines(), ["x"]);
    assert_eq!(editor.textarea.cursor().0, 0);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn an_outside_change_does_not_replace_a_buffer_with_edits() {
    let path = temp_file("watchdirty", "text\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    change_outside(&path, "outside\n", 5);

    editor.tick(Instant::now() + Duration::from_secs(2));
    assert_eq!(editor.textarea.lines(), ["Xtext"]);
    assert!(editor.dirty);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_file_is_checked_about_once_a_second() {
    let path = temp_file("watchrate", "a\n");
    let mut editor = open(&path);
    let start = Instant::now();
    editor.tick(start);
    change_outside(&path, "b\n", 5);

    editor.tick(start + Duration::from_millis(500));
    assert_eq!(editor.textarea.lines(), ["a"], "too soon to look again");
    editor.tick(start + Duration::from_millis(1100));
    assert_eq!(editor.textarea.lines(), ["b"]);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_deleted_file_is_reported_once_and_does_not_crash() {
    let path = temp_file("watchgone", "text\n");
    let mut editor = open(&path);
    fs::remove_file(&path).unwrap();

    assert!(editor.tick(Instant::now() + Duration::from_secs(2)));
    assert!(editor.message.contains("gone"), "{}", editor.message);
    assert!(
        !editor.tick(Instant::now() + Duration::from_secs(4)),
        "no repeat"
    );
    assert_eq!(editor.textarea.lines(), ["text"], "the buffer stays");
    // The file comes back: the editor loads it.
    fs::write(&path, "back\n").unwrap();
    editor.tick(Instant::now() + Duration::from_secs(6));
    assert_eq!(editor.textarea.lines(), ["back"]);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// Texts in other scripts. Each has the length of the first visible character, in code points.
/// `Right` moves over one visible character: for these texts, that is `first` code points.
const SCRIPTS: [(&str, &str, usize); 7] = [
    ("devanagari", "संस्कृतम्", 2),
    ("arabic", "العربية", 1),
    ("chinese", "中文", 1),
    ("precomposed", "\u{e9}t\u{e9}", 1),
    ("combining", "e\u{301}te\u{301}", 2),
    ("math", "\u{1d465} \u{2211}", 1),
    ("emoji", "👨\u{200d}👩\u{200d}👧\u{200d}👦", 7),
];

#[test]
fn a_save_keeps_text_in_other_scripts_byte_for_byte() {
    for (name, text, _) in SCRIPTS {
        let path = temp_file(&format!("script-{name}"), &format!("{text}\n"));
        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::Char('!')));
        editor.handle_key(key(KeyCode::Esc));
        assert_eq!(
            fs::read(&path).unwrap(),
            format!("!{text}\n").into_bytes(),
            "{name}"
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

#[test]
fn right_moves_over_one_visible_character() {
    for (name, text, first) in SCRIPTS {
        let path = temp_file(&format!("right-{name}"), &format!("{text}\n"));
        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::Right));
        assert_eq!(editor.cursor_position(), (0, first), "{name}");
        // Left goes back over the same character.
        editor.handle_key(key(KeyCode::Left));
        assert_eq!(editor.cursor_position(), (0, 0), "{name}: Left");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

#[test]
fn backspace_and_delete_remove_one_visible_character() {
    for (name, text, first) in SCRIPTS {
        let chars = text.chars().count();
        let path = temp_file(&format!("erase-{name}"), &format!("{text}\n"));
        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::Delete));
        assert_eq!(
            editor.textarea.lines()[0].chars().count(),
            chars - first,
            "{name}: Delete"
        );
        let mut editor = open(&path);
        editor.handle_key(key(KeyCode::End));
        editor.handle_key(key(KeyCode::Backspace));
        let left = editor.textarea.lines()[0].chars().count();
        assert!(left < chars && chars - left >= 1, "{name}: Backspace");
        assert!(text.starts_with(&editor.textarea.lines()[0]), "{name}");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

#[test]
fn shift_right_selects_one_visible_character_and_the_arrows_still_cross_lines() {
    let path = temp_file("shiftgraph", "e\u{301}x\nnext\n");
    let mut editor = open(&path);
    editor.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
    editor.handle_key(ctrl('c'));
    assert_eq!(editor.take_clipboard().as_deref(), Some("e\u{301}"));
    editor.handle_key(key(KeyCode::End));
    editor.handle_key(key(KeyCode::Right));
    assert_eq!(
        editor.cursor_position(),
        (1, 0),
        "Right at the end of a line"
    );
    editor.handle_key(key(KeyCode::Left));
    assert_eq!(
        editor.cursor_position(),
        (0, 3),
        "Left at the start of a line"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn wrapping_keeps_text_in_other_scripts_whole() {
    for (name, text, _) in SCRIPTS {
        let line = format!("{text} {text} {text} {text}");
        for width in [3, 5, 8, 20] {
            let rows = wrap_rows(&line, width);
            assert_eq!(
                rows.concat().replace(' ', ""),
                line.replace(' ', ""),
                "{name} at width {width}"
            );
        }
    }
}

#[test]
fn the_column_after_a_devanagari_word_is_the_column_of_typst() {
    // Typst counts code points from 1. The cursor column counts code points from 0, plus 1 on screen.
    let path = temp_file(
        "script-column",
        "\u{938}\u{902}\u{938}\u{94d}\u{915}\u{943}\u{924}\u{92e}\u{94d} #nope()\n",
    );
    let mut editor = open(&path);
    editor.start_compile();
    wait_for_report(&mut editor);
    editor.handle_key(ctrl('g'));
    let (row, column) = editor.cursor_position();
    assert_eq!((row, column), (0, 10), "{}", editor.message);
    assert!(editor.message.contains("1:11"), "{}", editor.message);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn ctrl_q_saves_and_quits() {
    let path = temp_file("ctrlq", "text\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    assert!(matches!(editor.handle_key(ctrl('q')), Action::Quit));
    assert_eq!(fs::read_to_string(&path).unwrap(), "Xtext\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn ctrl_q_in_a_conflict_warns_then_quits_and_keeps_the_disk_version() {
    let path = temp_file("ctrlqconflict", "text\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    fs::write(&path, "outside\n").unwrap();

    assert!(matches!(editor.handle_key(ctrl('q')), Action::Stay));
    assert!(
        editor.message.contains("changed on disk"),
        "{}",
        editor.message
    );
    assert!(
        editor.message.contains("Ctrl-Q again"),
        "{}",
        editor.message
    );
    assert!(matches!(editor.handle_key(ctrl('q')), Action::Quit));
    assert_eq!(fs::read_to_string(&path).unwrap(), "outside\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_paste_inserts_all_lines_at_once_and_one_undo_takes_it_back() {
    let path = temp_file("paste", "end\n");
    let mut editor = open(&path);
    let pasted: String = (0..1000).map(|n| format!("line {n}\n")).collect();
    editor.paste(&pasted);
    assert_eq!(editor.textarea.lines().len(), 1001);
    assert_eq!(editor.textarea.lines()[999], "line 999");
    assert!(editor.dirty && editor.last_edit.is_some());
    editor.handle_key(ctrl('u'));
    assert_eq!(editor.textarea.lines().len(), 1, "one undo step");
    assert_eq!(editor.textarea.lines()[0], "end");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_paste_keeps_tab_characters_and_turns_every_line_end_into_lf() {
    let path = temp_file("pastetab", "");
    let mut editor = open(&path);
    editor.paste("a\tb\r\nc\rd\ne");
    assert_eq!(editor.textarea.lines(), ["a\tb", "c", "d", "e"]);
    editor.handle_key(key(KeyCode::Esc));
    assert_eq!(fs::read(&path).unwrap(), b"a\tb\nc\nd\ne\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn ctrl_c_and_ctrl_x_put_the_selection_on_the_system_clipboard() {
    let path = temp_file("clip", "héllo wörld\n");
    let mut editor = open(&path);
    let shift = |code| KeyEvent::new(code, KeyModifiers::SHIFT);
    for _ in 0..5 {
        editor.handle_key(shift(KeyCode::Right));
    }
    editor.handle_key(ctrl('c'));
    assert_eq!(editor.take_clipboard().as_deref(), Some("héllo"));
    assert_eq!(editor.take_clipboard(), None, "taken once");
    assert_eq!(
        editor.textarea.lines()[0],
        "héllo wörld",
        "copy keeps the text"
    );

    editor.handle_key(key(KeyCode::Home));
    for _ in 0..5 {
        editor.handle_key(shift(KeyCode::Right));
    }
    editor.handle_key(ctrl('x'));
    assert_eq!(editor.take_clipboard().as_deref(), Some("héllo"));
    assert_eq!(editor.textarea.lines()[0], " wörld");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn ctrl_c_without_a_selection_sends_nothing() {
    let path = temp_file("clipnone", "text\n");
    let mut editor = open(&path);
    editor.handle_key(ctrl('c'));
    assert_eq!(editor.take_clipboard(), None);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn ctrl_o_before_an_export_says_so() {
    let path = temp_file("openpdf", "text\n");
    let mut editor = open(&path);
    editor.handle_key(ctrl('o'));
    assert!(editor.message.contains("No PDF yet"), "{}", editor.message);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_viewer_that_cannot_start_shows_an_error_and_a_viewer_that_starts_shows_the_pdf() {
    let path = temp_file("openpdf2", "text\n");
    let mut editor = open(&path);
    editor.exported = Some(path.with_extension("pdf"));
    editor.open_pdf("lazytypst-no-such-viewer");
    assert!(
        editor
            .message
            .starts_with("Cannot start lazytypst-no-such-viewer"),
        "{}",
        editor.message
    );
    editor.open_pdf("true");
    assert!(editor.message.starts_with("Opening "), "{}", editor.message);
    assert!(editor.message.ends_with("doc.pdf"), "{}", editor.message);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn with_the_live_compile_off_typing_saves_but_starts_no_compile() {
    let path = temp_file("paused", "text\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::F(5)));
    assert!(editor.paused);
    assert!(screen_text(&mut editor).contains("Compile (paused)"));
    editor.handle_key(key(KeyCode::Char('X')));
    editor.tick(Instant::now() + Duration::from_secs(1));
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "Xtext\n",
        "the autosave runs"
    );
    assert!(editor.job.is_none(), "no compile starts");

    editor.handle_key(ctrl('b'));
    assert!(editor.job.is_some(), "Ctrl-B still compiles");
    editor.stop_compile();

    editor.handle_key(key(KeyCode::F(5)));
    assert!(!editor.paused);
    editor.handle_key(key(KeyCode::Char('Y')));
    editor.tick(Instant::now() + Duration::from_secs(1));
    assert!(editor.job.is_some(), "the next pause compiles");
    editor.stop_compile();
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// Each editing key that the README and the help text list. If a key changes in `ratatui-textarea`,
/// this test fails, and the list must change.
#[test]
fn the_listed_editing_keys_work_as_listed() {
    let alt = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT);
    let text = "one two three\nsecond line\nthird\n";
    let path = temp_file("editkeys", text);
    let mut editor = open(&path);
    let lines = |editor: &Editor| editor.textarea.lines().join("/");
    let at = |editor: &mut Editor, row: u16, column: u16| {
        editor.textarea.move_cursor(CursorMove::Jump(row, column));
    };

    editor.handle_key(key(KeyCode::Char('X')));
    editor.handle_key(ctrl('u'));
    assert_eq!(
        lines(&editor),
        "one two three/second line/third",
        "Ctrl-U undoes"
    );
    editor.handle_key(ctrl('r'));
    assert_eq!(
        lines(&editor),
        "Xone two three/second line/third",
        "Ctrl-R redoes"
    );
    editor.handle_key(ctrl('u'));

    at(&mut editor, 0, 13);
    editor.handle_key(ctrl('w'));
    assert_eq!(lines(&editor), "one two /second line/third", "Ctrl-W");
    editor.handle_key(ctrl('u'));

    at(&mut editor, 0, 4);
    editor.handle_key(ctrl('k'));
    assert_eq!(lines(&editor), "one /second line/third", "Ctrl-K");
    editor.handle_key(ctrl('u'));

    at(&mut editor, 0, 4);
    editor.handle_key(ctrl('j'));
    assert_eq!(lines(&editor), "two three/second line/third", "Ctrl-J");
    editor.handle_key(ctrl('u'));

    at(&mut editor, 0, 4);
    editor.handle_key(alt('f'));
    assert_eq!(editor.cursor_position(), (0, 8), "Alt-F");
    editor.handle_key(alt('b'));
    assert_eq!(editor.cursor_position(), (0, 4), "Alt-B");
    editor.handle_key(ctrl('a'));
    assert_eq!(editor.cursor_position(), (0, 0), "Ctrl-A");
    editor.handle_key(key(KeyCode::End));
    assert_eq!(editor.cursor_position(), (0, 13), "End");
    editor.handle_key(alt('>'));
    assert_eq!(editor.cursor_position().0, 2, "Alt-> goes to the last line");
    editor.handle_key(alt('<'));
    assert_eq!(
        editor.cursor_position().0,
        0,
        "Alt-< goes to the first line"
    );

    // Select with Shift and the arrows, copy with Ctrl-C, paste with Ctrl-Y.
    at(&mut editor, 0, 0);
    for _ in 0..3 {
        editor.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
    }
    editor.handle_key(ctrl('c'));
    editor.handle_key(key(KeyCode::End));
    editor.handle_key(ctrl('y'));
    assert_eq!(
        lines(&editor),
        "one two threeone/second line/third",
        "Ctrl-C, Ctrl-Y"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn ctrl_v_and_alt_v_scroll_one_page() {
    let text: String = (0..100).map(|n| format!("line {n}\n")).collect();
    let path = temp_file("editpages", &text);
    let mut editor = open(&path);
    let _ = screen_text(&mut editor); // the text area learns the height of the screen
    editor.handle_key(ctrl('v'));
    let down = editor.cursor_position().0;
    assert!(down > 5, "Ctrl-V moves down a page: {down}");
    editor.handle_key(ctrl('v'));
    editor.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::ALT));
    assert!(editor.cursor_position().0 < 2 * down, "Alt-V moves up");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

fn type_in_search(editor: &mut Editor, text: &str) {
    for letter in text.chars() {
        editor.handle_key(key(KeyCode::Char(letter)));
    }
}

#[test]
fn search_moves_to_the_next_match_wraps_and_the_same_key_goes_on() {
    let path = temp_file("search", "Item one\nsecond\nItem two\nthird Item\n");
    let mut editor = open(&path);
    editor.handle_key(ctrl('f'));
    assert!(
        status_row(&mut editor).starts_with("Search (text):"),
        "{}",
        status_row(&mut editor)
    );
    type_in_search(&mut editor, "Item");
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(
        editor.cursor_position(),
        (2, 0),
        "the first match after the cursor"
    );
    editor.handle_key(ctrl('f'));
    assert_eq!(editor.cursor_position(), (3, 6), "Ctrl-F goes on");
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(editor.cursor_position(), (0, 0), "it wraps to the top");
    editor.handle_key(key(KeyCode::Esc));
    assert!(matches!(editor.mode, Mode::Edit));
    assert_eq!(editor.cursor_position(), (0, 0), "Esc keeps the cursor");
    assert!(matches!(
        editor.handle_key(key(KeyCode::Esc)),
        Action::Close
    ));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_pattern_with_no_match_says_so_and_the_cursor_stays() {
    let path = temp_file("searchnone", "alpha\nbeta\n");
    let mut editor = open(&path);
    editor.textarea.move_cursor(CursorMove::Jump(1, 2));
    editor.handle_key(ctrl('f'));
    type_in_search(&mut editor, "zzz");
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(editor.message, "No match for zzz");
    assert_eq!(editor.cursor_position(), (1, 2));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn an_invalid_pattern_shows_the_error_and_does_not_crash() {
    let path = temp_file("searchbad", "alpha (x\n");
    let mut editor = open(&path);
    editor.handle_key(ctrl('f'));
    editor.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::ALT)); // regular expression mode
    type_in_search(&mut editor, "(");
    assert!(
        editor.message.starts_with("Invalid pattern:"),
        "{}",
        editor.message
    );
    editor.handle_key(key(KeyCode::Enter));
    assert!(editor.message.starts_with("Invalid pattern:"));
    editor.handle_key(key(KeyCode::Backspace));
    type_in_search(&mut editor, "\\(");
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(
        editor.cursor_position(),
        (0, 6),
        "an escaped bracket matches"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_prompt_takes_every_key_and_the_text_stays_unchanged() {
    let path = temp_file("searchkeys", "text\n");
    let mut editor = open(&path);
    editor.handle_key(ctrl('f'));
    type_in_search(&mut editor, "abc");
    assert_eq!(editor.textarea.lines(), ["text"]);
    assert!(!editor.dirty);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// Draws the editor on a `width` by 20 screen and returns the rows.
fn rows_at(editor: &mut Editor, width: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, 20)).unwrap();
    terminal.draw(|frame| editor.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..20)
        .map(|row| {
            (0..width)
                .map(|column| buffer[(column, row)].symbol())
                .collect()
        })
        .collect()
}

#[test]
fn f11_shows_the_preview_on_the_full_width_and_the_next_f11_shows_the_layout_again() {
    let path = temp_file("full", "hello text\n");
    let mut editor = open(&path);
    let normal = rows_at(&mut editor, 100);
    assert!(normal[0].contains("doc.typ"), "{}", normal[0]);
    assert!(normal[0].contains("Preview"), "{}", normal[0]);
    assert!(
        normal[0].find("Preview") > Some(40),
        "the preview is the right half"
    );

    editor.handle_key(key(KeyCode::F(11)));
    let full = rows_at(&mut editor, 100);
    assert!(full[0].starts_with("┌Preview"), "{}", full[0]);
    assert!(
        !full.iter().any(|row| row.contains("hello text")),
        "the text is hidden"
    );
    assert!(full[19].contains("F11 back to editor"), "{}", full[19]);
    assert!(full[19].trim_end().ends_with("100%"), "{}", full[19]);

    editor.handle_key(key(KeyCode::F(11)));
    assert_eq!(rows_at(&mut editor, 100), normal);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn typing_does_nothing_in_the_full_preview_and_esc_goes_back_to_the_editor_not_the_list() {
    let path = temp_file("fulltype", "text\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::F(11)));
    for letter in "abc".chars() {
        editor.handle_key(key(KeyCode::Char(letter)));
    }
    editor.handle_key(ctrl('x'));
    assert_eq!(editor.textarea.lines(), ["text"]);
    assert!(!editor.dirty);
    assert!(matches!(editor.handle_key(key(KeyCode::Esc)), Action::Stay));
    assert!(
        matches!(editor.mode, Mode::Edit),
        "Esc shows the editor again"
    );
    assert!(matches!(
        editor.handle_key(key(KeyCode::Esc)),
        Action::Close
    ));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_page_keys_work_in_the_full_preview() {
    let path = temp_file("fullpages", "= One\n#pagebreak()\n= Two\n");
    let mut editor = open(&path);
    compile_and_wait(&mut editor);
    editor.handle_key(key(KeyCode::F(11)));
    editor.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    assert_eq!(editor.preview.wanted_page(), 2);
    editor.stop_compile();
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn plus_zooms_a_compile_starts_at_the_new_resolution_and_zero_fits_the_page() {
    let path = temp_file("fullzoom", "= One\n");
    let mut editor = open(&path);
    compile_and_wait(&mut editor);
    editor.handle_key(key(KeyCode::F(11)));
    editor.handle_key(key(KeyCode::Char('+')));
    editor.handle_key(key(KeyCode::Char('+')));
    assert_eq!(editor.preview.zoom_percent(), 200);
    assert!(editor.job.is_some(), "the new zoom needs a new compile");
    wait_for_report(&mut editor);
    let rows = rows_at(&mut editor, 100);
    assert!(rows[0].contains("Preview 1/1 200%"), "{}", rows[0]);
    assert!(rows[19].trim_end().ends_with("200%"), "{}", rows[19]);

    editor.handle_key(key(KeyCode::Down));
    assert_eq!(editor.preview.pan(), (0.0, 0.25));
    // A new compile keeps the zoom and the place. Ctrl-B works in the full preview.
    editor.handle_key(ctrl('b'));
    wait_for_report(&mut editor);
    assert_eq!(editor.preview.zoom_percent(), 200);
    assert_eq!(editor.preview.pan(), (0.0, 0.25));

    editor.handle_key(key(KeyCode::Char('0')));
    assert_eq!(editor.preview.zoom_percent(), 100);
    wait_for_report(&mut editor);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// The foreground color of the first cell on screen whose symbol starts a row text `word`, in the
/// text area half of a 100 by 20 screen. Returns the cell.
fn cell_of(editor: &mut Editor, word: &str) -> ratatui::buffer::Cell {
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| editor.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    for row in 0..20 {
        let text: String = (0..50)
            .map(|column| buffer[(column, row)].symbol())
            .collect();
        if let Some(at) = text.find(word) {
            let column = text[..at].chars().count();
            return buffer[(u16::try_from(column).unwrap(), row)].clone();
        }
    }
    panic!("{word:?} is not on the screen");
}

#[test]
fn the_editor_colors_headings_commands_strings_math_and_comments() {
    let text = "= Heading\n#set text(font: \"Libertinus\")\nSome $x + y$ and // a note\n";
    let path = temp_file("syntax", text);
    let mut editor = open(&path);
    editor.colors = true;
    editor.textarea.move_cursor(CursorMove::Jump(2, 0));
    assert_eq!(cell_of(&mut editor, "Heading").fg, Color::Blue);
    assert_eq!(cell_of(&mut editor, "#set").fg, Color::Magenta);
    assert_eq!(cell_of(&mut editor, "\"Libertinus").fg, Color::Green);
    assert_eq!(
        cell_of(&mut editor, "text(").fg,
        Color::Reset,
        "plain code stays plain"
    );
    assert_eq!(cell_of(&mut editor, "$x").fg, Color::Yellow);
    assert_eq!(cell_of(&mut editor, "// a note").fg, Color::DarkGray);
    assert_eq!(cell_of(&mut editor, "Some").fg, Color::Reset);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_colors_follow_the_text_after_a_wrap_and_a_tab() {
    // The comment is after the wrap of a long line.
    let text = format!("{}// the comment\n\t#set x\n", "word ".repeat(11));
    let path = temp_file("syntaxwrap", &text);
    let mut editor = open(&path);
    editor.colors = true;
    editor.textarea.move_cursor(CursorMove::Jump(1, 5));
    assert_eq!(cell_of(&mut editor, "// the comment").fg, Color::DarkGray);
    assert_eq!(
        cell_of(&mut editor, "#set").fg,
        Color::Magenta,
        "after a tab"
    );
    assert_eq!(cell_of(&mut editor, "word word").fg, Color::Reset);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn without_colors_the_parts_use_bold_dim_and_italic_only() {
    let path = temp_file("syntaxplain", "= Heading\n// note\n$x$ text\n");
    let mut editor = open(&path);
    editor.colors = false;
    editor.textarea.move_cursor(CursorMove::Jump(2, 6));
    let heading = cell_of(&mut editor, "Heading");
    assert_eq!(heading.fg, Color::Reset);
    assert!(heading.modifier.contains(ratatui::style::Modifier::BOLD));
    let comment = cell_of(&mut editor, "// note");
    assert_eq!(comment.fg, Color::Reset);
    assert!(comment.modifier.contains(ratatui::style::Modifier::DIM));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_color_step_is_cheap_on_a_file_of_2000_lines() {
    let text: String = (0..2000)
        .map(|n| format!("= Heading {n}\nText with $x$ and #emph[word] and \"q\" // note\n"))
        .collect();
    let path = temp_file("syntaxspeed", &text);
    let mut editor = open(&path);
    editor.colors = true;
    let area = Rect::new(0, 0, 50, 20);
    let mut terminal = Terminal::new(TestBackend::new(100, 22)).unwrap();
    let draw = |editor: &mut Editor, terminal: &mut Terminal<TestBackend>| {
        let start = Instant::now();
        terminal.draw(|frame| editor.draw(frame)).unwrap();
        start.elapsed()
    };
    let whole = draw(&mut editor, &mut terminal);
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    let start = Instant::now();
    editor.paint(&mut buffer, area);
    let paint = start.elapsed();
    eprintln!("draw of 4000 lines: {whole:?}, of which the color step: {paint:?}");
    assert!(paint < Duration::from_millis(100), "{paint:?}");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_paste_does_not_reach_the_text_while_the_search_prompt_or_the_full_preview_is_open() {
    let path = temp_file("pastemodes", "hello\n");
    let mut editor = open(&path);
    editor.handle_key(ctrl('f'));
    editor.paste("XY\nZ");
    assert_eq!(
        editor.textarea.lines(),
        ["hello"],
        "the prompt takes the paste"
    );
    let Mode::Search(prompt) = &editor.mode else {
        panic!("the prompt must be open");
    };
    assert_eq!(prompt.lines(), ["XYZ"]);
    assert!(!editor.dirty);
    editor.handle_key(key(KeyCode::Esc));
    editor.handle_key(key(KeyCode::F(11)));
    editor.paste("XYZ");
    assert_eq!(
        editor.textarea.lines(),
        ["hello"],
        "typing is off in the full preview"
    );
    assert!(!editor.dirty);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn backspace_with_a_selection_deletes_only_the_selection() {
    let path = temp_file("selback", "e\u{301}ab\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::End));
    for _ in 0..2 {
        editor.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
    }
    editor.handle_key(key(KeyCode::Backspace));
    assert_eq!(editor.textarea.lines(), ["e\u{301}"]);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_colors_follow_a_tab_that_is_not_at_a_tab_stop_and_a_combining_mark() {
    let path = temp_file("paintwidths", "a\t#set x\ne\u{301}#set y\n");
    let mut editor = open(&path);
    editor.colors = true;
    editor.textarea.move_cursor(CursorMove::Jump(1, 7));
    assert_eq!(
        cell_of(&mut editor, "#set x").fg,
        Color::Magenta,
        "after a short tab"
    );
    assert_eq!(
        cell_of(&mut editor, "#set y").fg,
        Color::Magenta,
        "after a combining mark"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn leaving_the_full_preview_shows_the_whole_page_again_and_starts_a_compile() {
    let path = temp_file("fullzoomexit", "= One\n");
    let mut editor = open(&path);
    compile_and_wait(&mut editor);
    editor.handle_key(key(KeyCode::F(11)));
    editor.handle_key(key(KeyCode::Char('+')));
    wait_for_report(&mut editor);
    assert_eq!(editor.preview.zoom_percent(), 150);
    editor.handle_key(key(KeyCode::F(11)));
    assert_eq!(editor.preview.zoom_percent(), 100);
    assert!(
        editor.job.is_some(),
        "the page is rendered at the normal resolution"
    );
    wait_for_report(&mut editor);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_reload_keeps_the_search_style_and_the_pattern_of_an_open_prompt() {
    let path = temp_file("reloadsearch", "alpha beta\n");
    let mut editor = open(&path);
    editor.handle_key(ctrl('f'));
    type_in_search(&mut editor, "beta");
    change_outside(&path, "alpha beta gamma\n", 5);
    editor.tick(Instant::now() + Duration::from_secs(2));
    assert_eq!(editor.textarea.lines(), ["alpha beta gamma"]);
    assert_eq!(
        editor.textarea.search_pattern().map(|p| p.as_str()),
        Some("(?i)beta")
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn f5_turns_the_live_compile_on_and_a_compile_starts_at_once() {
    let path = temp_file("f5on", "= One\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::F(5)));
    assert!(editor.job.is_none());
    editor.handle_key(key(KeyCode::F(5)));
    assert!(editor.job.is_some(), "the preview follows the text again");
    editor.stop_compile();
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_file_with_crlf_line_ends_keeps_them_after_a_save() {
    let path = temp_file("crlf", "one\r\ntwo\r\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    editor.handle_key(key(KeyCode::Esc));
    assert_eq!(fs::read(&path).unwrap(), b"Xone\r\ntwo\r\n");
    // A file with LF line ends stays LF.
    fs::write(&path, "one\ntwo\n").unwrap();
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    editor.handle_key(key(KeyCode::Esc));
    assert_eq!(fs::read(&path).unwrap(), b"Xone\ntwo\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn closing_the_editor_deletes_its_page_folders() {
    let path = temp_file("dropdirs", "= One\n");
    let mut editor = open(&path);
    compile_and_wait(&mut editor);
    let dir = editor.preview.dir().unwrap().to_path_buf();
    assert!(dir.exists());
    editor.start_compile();
    let running = editor.job.as_ref().unwrap().output().to_path_buf();
    drop(editor);
    assert!(!dir.exists(), "the folder of the page on screen");
    assert!(!running.exists(), "the folder of the running compile");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_keys_that_save_compile_and_quit_work_in_the_full_preview_and_typing_does_not() {
    let path = temp_file("fullkeys", "text\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    editor.handle_key(key(KeyCode::F(11)));
    editor.handle_key(ctrl('s'));
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "Xtext\n",
        "Ctrl-S saves"
    );
    editor.handle_key(key(KeyCode::Char('Y')));
    assert_eq!(editor.textarea.lines(), ["Xtext"], "a letter does nothing");
    assert!(matches!(editor.handle_key(ctrl('q')), Action::Quit));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_search_prompt_and_the_full_preview_exclude_each_other() {
    let path = temp_file("modes", "hello\n");
    let mut editor = open(&path);
    editor.handle_key(ctrl('f'));
    editor.handle_key(key(KeyCode::F(11)));
    assert!(
        matches!(editor.mode, Mode::Search(_)),
        "F11 does not leave the prompt"
    );
    editor.handle_key(key(KeyCode::Esc));
    assert!(matches!(editor.mode, Mode::Edit));

    editor.handle_key(key(KeyCode::F(11)));
    assert!(matches!(editor.mode, Mode::Full));
    editor.handle_key(ctrl('f'));
    assert!(
        matches!(editor.mode, Mode::Full),
        "Ctrl-F does not open the prompt in the full preview"
    );
    editor.handle_key(key(KeyCode::F(11)));
    assert!(matches!(editor.mode, Mode::Edit));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// What a key can change in the editor. A key that changes none of these did nothing.
#[derive(Debug, PartialEq)]
struct Snapshot {
    text: Vec<String>,
    mode: &'static str,
    cursor: (usize, usize),
    message: String,
    compiling: bool,
    exporting: bool,
    paused: bool,
    page: usize,
    left: bool,
}

fn snapshot(editor: &Editor, left: bool) -> Snapshot {
    Snapshot {
        text: editor.textarea.lines().to_vec(),
        mode: match editor.mode {
            Mode::Edit => "edit",
            Mode::Search(_) => "search",
            Mode::Line(_) => "line",
            Mode::Replace(_) => "replace",
            Mode::Outline(_) => "outline",
            Mode::History(_) => "history",
            Mode::Spell(_) => "spell",
            Mode::Full => "full",
        },
        cursor: editor.cursor_position(),
        message: editor.message.clone(),
        compiling: editor.job.is_some(),
        exporting: editor.export.is_some(),
        paused: editor.paused,
        page: editor.preview.wanted_page(),
        left,
    }
}

/// An editor in the mode `mode`. With `with_page`, a compile of page 2 of 3 has finished, so that the
/// page keys have a page to turn. Without it, no compile runs, which keeps the test fast.
fn editor_in_mode(name: &str, mode: &str, with_page: bool) -> (Editor, PathBuf) {
    let path = temp_file(name, "= One\n#pagebreak()\n= Two\n#pagebreak()\n= Three\n");
    let mut editor = open(&path);
    if with_page {
        editor.preview.want(2);
        editor.start_compile();
        wait_for_report(&mut editor);
        assert_eq!(editor.preview.wanted_page(), 2);
    }
    match mode {
        "search" => {
            editor.handle_key(ctrl('f'));
        }
        "full" => {
            editor.handle_key(key(KeyCode::F(11)));
        }
        _ => {}
    }
    (editor, path)
}

fn key_event(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

/// Each key of the editor in the help list works in the text, is typed into the prompt in the search
/// mode, and works or is ignored on purpose in the full preview. If a key drifts, this test names it.
#[test]
fn every_key_of_the_help_list_does_what_the_mode_says() {
    use crate::help::{KEYS, Scope};
    let mut ignored_in_full = Vec::new();
    for entry in KEYS.iter().filter(|entry| entry.scope == Scope::Editor) {
        let Some((code, modifiers)) = entry.press else {
            continue;
        };
        for mode in ["edit", "search", "full"] {
            let page_key = modifiers.contains(KeyModifiers::ALT);
            let (mut editor, path) = editor_in_mode(&format!("allkeys-{mode}"), mode, page_key);
            let before = snapshot(&editor, false);
            let action = editor.handle_key(key_event(code, modifiers));
            let after = snapshot(&editor, !matches!(action, Action::Stay));
            let acted = before != after;
            match mode {
                "edit" => assert!(acted, "{} does nothing in the text", entry.keys),
                "search" => {
                    assert_eq!(
                        before.text, after.text,
                        "{} changes the text in the search prompt",
                        entry.keys
                    );
                    assert!(
                        matches!(action, Action::Stay),
                        "{} acts in the search prompt",
                        entry.keys
                    );
                }
                _ => {
                    if !acted {
                        ignored_in_full.push(entry.keys);
                    }
                    assert_eq!(
                        before.text, after.text,
                        "{} changes the text in the full preview",
                        entry.keys
                    );
                }
            }
            drop(editor);
            fs::remove_dir_all(path.parent().unwrap()).unwrap();
        }
    }
    // Only the search key does nothing in the full preview. A new entry in this list is a decision.
    assert_eq!(
        ignored_in_full,
        [
            "Ctrl-F",
            "Alt-Enter, Ctrl-]",
            "F3",
            "F6",
            "F7",
            "Alt-;",
            "F10",
            "Alt-S",
            "Alt-G"
        ]
    );
}

#[test]
fn shift_tab_removes_one_indent_level_and_keeps_the_cursor_in_the_text() {
    let path = temp_file("dedent", "      deep\n one\nnone\n\tcode\n");
    let mut editor = open(&path);
    let back_tab = key(KeyCode::BackTab);
    editor.textarea.move_cursor(CursorMove::Jump(0, 8));
    editor.handle_key(back_tab);
    assert_eq!(editor.textarea.lines()[0], "    deep", "two spaces go");
    assert_eq!(
        editor.cursor_position(),
        (0, 6),
        "the cursor stays on the same letter"
    );
    assert!(editor.dirty);
    editor.handle_key(ctrl('u'));
    assert_eq!(editor.textarea.lines()[0], "      deep", "one undo step");

    editor.textarea.move_cursor(CursorMove::Jump(1, 2));
    editor.handle_key(back_tab);
    assert_eq!(
        editor.textarea.lines()[1],
        "one",
        "one space is all there is"
    );

    editor.textarea.move_cursor(CursorMove::Jump(2, 1));
    editor.dirty = false;
    editor.handle_key(back_tab);
    assert_eq!(
        editor.textarea.lines()[2],
        "none",
        "no indent: nothing changes"
    );
    assert!(!editor.dirty, "and nothing is marked as edited");

    editor.textarea.move_cursor(CursorMove::Jump(3, 3));
    editor.handle_key(back_tab);
    assert_eq!(editor.textarea.lines()[3], "code", "a tab character goes");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn enter_keeps_the_indent_of_the_line_and_adds_a_level_after_an_opening_bracket() {
    let path = temp_file(
        "autoindent",
        "  alpha beta\nplain\n  #let f(x) = {\n\tcode\n",
    );
    let mut editor = open(&path);
    let enter = key(KeyCode::Enter);
    // At the end of an indented line.
    editor.textarea.move_cursor(CursorMove::Jump(0, 12));
    editor.handle_key(enter);
    assert_eq!(editor.textarea.lines()[1], "  ");
    assert_eq!(editor.cursor_position(), (1, 2));
    editor.handle_key(ctrl('u'));
    assert_eq!(
        editor.textarea.lines().len(),
        4,
        "one undo step takes back the break and the indent"
    );
    // In the middle of the line: the rest moves down with the indent.
    editor.textarea.move_cursor(CursorMove::Jump(0, 8));
    editor.handle_key(enter);
    assert_eq!(&editor.textarea.lines()[..2], ["  alpha ", "  beta"]);
    editor.handle_key(ctrl('u'));
    // No indent: a plain break.
    editor.textarea.move_cursor(CursorMove::Jump(1, 5));
    editor.handle_key(enter);
    assert_eq!(editor.textarea.lines()[2], "");
    editor.handle_key(ctrl('u'));
    // After an opening bracket: one more level.
    editor.textarea.move_cursor(CursorMove::Jump(2, 15));
    editor.handle_key(enter);
    assert_eq!(editor.textarea.lines()[3], "    ");
    assert_eq!(editor.cursor_position(), (3, 4));
    editor.handle_key(ctrl('u'));
    // Inside the indent itself, the new line gets the indent up to the cursor.
    editor.textarea.move_cursor(CursorMove::Jump(2, 1));
    editor.handle_key(enter);
    // The line below keeps its whole indent: one space from the new indent, one that was after the cursor.
    assert_eq!(&editor.textarea.lines()[2..4], [" ", "  #let f(x) = {"]);
    editor.handle_key(ctrl('u'));
    // A tab character is copied as it is, and a bracket adds a tab.
    editor.textarea.move_cursor(CursorMove::Jump(3, 5));
    editor.handle_key(enter);
    assert_eq!(editor.textarea.lines()[4], "\t");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn enter_with_a_selection_replaces_it_without_an_indent() {
    let path = temp_file("autoindentsel", "  abc def\n");
    let mut editor = open(&path);
    editor.textarea.move_cursor(CursorMove::Jump(0, 2));
    for _ in 0..3 {
        editor.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
    }
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(editor.textarea.lines(), ["  ", " def"]);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn search_finds_plain_text_with_special_characters() {
    let path = temp_file("searchplain", "see f(x) and 1+1 and $x$ and a.b and [z]\n");
    let mut editor = open(&path);
    for (needle, column) in [
        ("f(x)", 4),
        ("1+1", 13),
        ("$x$", 21),
        ("a.b", 29),
        ("[z]", 37),
    ] {
        editor.textarea.move_cursor(CursorMove::Jump(0, 0));
        editor.handle_key(ctrl('f'));
        // The prompt starts with the last text: clear it.
        for _ in 0..20 {
            editor.handle_key(key(KeyCode::Backspace));
        }
        type_in_search(&mut editor, needle);
        assert!(
            !editor.message.starts_with("Invalid"),
            "{needle}: {}",
            editor.message
        );
        editor.handle_key(key(KeyCode::Enter));
        assert_eq!(editor.cursor_position(), (0, column), "{needle}");
        editor.handle_key(key(KeyCode::Esc));
    }
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn search_ignores_case_for_lowercase_text_and_respects_it_for_text_with_a_capital() {
    let path = temp_file("searchcase", "an item and an Item\n");
    let mut editor = open(&path);
    editor.handle_key(ctrl('f'));
    type_in_search(&mut editor, "item");
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(editor.cursor_position(), (0, 3), "item finds item first");
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(editor.cursor_position(), (0, 15), "and then Item");
    editor.handle_key(key(KeyCode::Esc));

    editor.textarea.move_cursor(CursorMove::Jump(0, 0));
    editor.handle_key(ctrl('f'));
    for _ in 0..10 {
        editor.handle_key(key(KeyCode::Backspace));
    }
    type_in_search(&mut editor, "Item");
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(editor.cursor_position(), (0, 15), "Item does not find item");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn alt_r_in_the_prompt_switches_between_text_and_regular_expression() {
    let path = temp_file("searchregex", "a.c abc\n");
    let mut editor = open(&path);
    editor.handle_key(ctrl('f'));
    assert!(
        status_row(&mut editor).starts_with("Search (text):"),
        "{}",
        status_row(&mut editor)
    );
    type_in_search(&mut editor, "a.c");
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(editor.cursor_position(), (0, 0), "text: the dot is a dot");
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(editor.cursor_position(), (0, 0), "text: no other a.c");
    editor.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::ALT));
    assert!(
        status_row(&mut editor).starts_with("Search (regex):"),
        "{}",
        status_row(&mut editor)
    );
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(
        editor.cursor_position(),
        (0, 4),
        "regex: the dot is any letter"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn alt_shift_with_an_arrow_does_not_turn_the_page() {
    let path = temp_file("altshift", "= One\n#pagebreak()\n= Two\n");
    let mut editor = open(&path);
    compile_and_wait(&mut editor);
    let alt_shift = KeyModifiers::ALT | KeyModifiers::SHIFT;
    for code in [KeyCode::Down, KeyCode::Up, KeyCode::Home, KeyCode::End] {
        editor.handle_key(KeyEvent::new(code, alt_shift));
        assert_eq!(editor.preview.wanted_page(), 1, "{code:?}");
        assert!(editor.job.is_none(), "{code:?} must not start a compile");
    }
    editor.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    assert_eq!(
        editor.preview.wanted_page(),
        2,
        "plain Alt-Down still turns"
    );
    editor.stop_compile();
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// The cell of the first digit of the number of the line `number` (counted from 1), for a screen of 100 by 20.
fn gutter_cell(editor: &mut Editor, number: usize) -> ratatui::buffer::Cell {
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| editor.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    for row in 0..20 {
        // The gutter starts after the border: a space, the number, a space.
        let text: String = (1..4)
            .map(|column| buffer[(column, row)].symbol())
            .collect();
        if text.trim() == number.to_string() {
            let column =
                1 + u16::try_from(text.find(|c: char| c.is_ascii_digit()).unwrap()).unwrap();
            return buffer[(column, row)].clone();
        }
    }
    panic!("no line number {number} on the screen");
}

#[test]
fn the_gutter_marks_the_lines_with_an_error_and_with_a_warning() {
    let path = temp_file("gutter", "one\ntwo\nthree\nfour\n");
    let mut editor = open(&path);
    editor.colors = true;
    editor.report = report_with(
        false,
        &[
            "doc.typ:2:1: error: boom",
            "doc.typ:4:1: warning: careful",
            "doc.typ:4:2: error: and an error on the same line",
            "other.typ:3:1: error: in another file",
            "hint: no place",
        ],
    );
    let plain = gutter_cell(&mut editor, 1);
    assert_eq!(plain.fg, Color::Reset);
    assert!(
        plain.modifier.contains(ratatui::style::Modifier::DIM),
        "a line with no mark stays dim"
    );
    let error = gutter_cell(&mut editor, 2);
    assert_eq!(error.fg, Color::Red);
    assert!(error.modifier.contains(ratatui::style::Modifier::BOLD));
    assert!(!error.modifier.contains(ratatui::style::Modifier::DIM));
    assert_eq!(
        gutter_cell(&mut editor, 3).fg,
        Color::Reset,
        "an error in another file marks nothing here"
    );
    assert_eq!(
        gutter_cell(&mut editor, 4).fg,
        Color::Red,
        "the error wins over the warning"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn without_colors_the_gutter_marks_use_reverse_and_underline() {
    let path = temp_file("guttermono", "one\ntwo\nthree\n");
    let mut editor = open(&path);
    editor.colors = false;
    editor.report = report_with(
        false,
        &["doc.typ:1:1: error: boom", "doc.typ:3:1: warning: careful"],
    );
    let error = gutter_cell(&mut editor, 1);
    assert_eq!(error.fg, Color::Reset);
    assert!(error.modifier.contains(ratatui::style::Modifier::REVERSED));
    let warning = gutter_cell(&mut editor, 3);
    assert!(
        warning
            .modifier
            .contains(ratatui::style::Modifier::UNDERLINED)
    );
    assert!(
        gutter_cell(&mut editor, 2)
            .modifier
            .contains(ratatui::style::Modifier::DIM)
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn f8_and_shift_f8_go_through_the_errors_in_order_and_wrap() {
    let path = temp_file(
        "nexterror",
        "one\ntwo is long\nthree\nfour\nfive\nsix\nseven\n",
    );
    let mut editor = open(&path);
    editor.report = report_with(
        false,
        &[
            "doc.typ:2:5: error: first",
            "doc.typ:2:9: warning: not an error",
            "doc.typ:4:1: error: second",
            "doc.typ:6:3: error: third",
        ],
    );
    let f8 = key(KeyCode::F(8));
    let shift_f8 = KeyEvent::new(KeyCode::F(8), KeyModifiers::SHIFT);
    // From the start of the file: the first error after the cursor.
    editor.handle_key(f8);
    assert_eq!(editor.cursor_position(), (1, 4));
    assert!(
        editor.message.starts_with("Error 1 of 3 at 2:5: first"),
        "{}",
        editor.message
    );
    editor.handle_key(f8);
    assert_eq!(editor.cursor_position(), (3, 0));
    editor.handle_key(f8);
    assert_eq!(editor.cursor_position(), (5, 2));
    editor.handle_key(f8);
    assert_eq!(editor.cursor_position(), (1, 4), "wraps to the first");
    assert!(
        editor.message.starts_with("Error 1 of 3"),
        "{}",
        editor.message
    );
    editor.handle_key(shift_f8);
    assert_eq!(editor.cursor_position(), (5, 2), "back wraps to the last");
    editor.handle_key(shift_f8);
    assert_eq!(editor.cursor_position(), (3, 0));
    // From a place between two errors.
    editor.textarea.move_cursor(CursorMove::Jump(4, 0));
    editor.handle_key(f8);
    assert_eq!(
        editor.cursor_position(),
        (5, 2),
        "the next error after line 5"
    );
    editor.textarea.move_cursor(CursorMove::Jump(4, 0));
    editor.handle_key(shift_f8);
    assert_eq!(
        editor.cursor_position(),
        (3, 0),
        "the previous error before line 5"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn f8_without_an_error_says_so_and_an_error_in_another_file_opens_it_after_a_save() {
    let path = temp_file("nexterror2", "one\ntwo\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::F(8)));
    assert!(editor.message.contains("No error"), "{}", editor.message);
    let dir = path.parent().unwrap();
    fs::write(dir.join("other.typ"), "x\ny\n").unwrap();
    editor.report = report_with(false, &["other.typ:2:1: error: elsewhere"]);
    editor.handle_key(key(KeyCode::Char('X')));
    let action = editor.handle_key(key(KeyCode::F(8)));
    assert!(
        matches!(&action, Action::Goto { file, line: 2, column: 1 } if file == Path::new("other.typ")),
        "the error is in other.typ"
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "Xone\ntwo\n",
        "saved before the switch"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_page_from_an_older_compile_says_old_in_the_title_while_the_last_compile_failed() {
    let path = temp_file("oldpage", "= One\n");
    let mut editor = open(&path);
    compile_and_wait(&mut editor);
    let title = |editor: &mut Editor| screen_rows(editor)[0].clone();
    assert!(
        title(&mut editor).contains("Preview 1/1"),
        "{}",
        title(&mut editor)
    );
    assert!(
        !title(&mut editor).contains("(old)"),
        "a good compile is not old"
    );

    editor.report = report_with(false, &["doc.typ:1:1: error: boom"]);
    let old = title(&mut editor);
    assert!(old.contains("Preview 1/1 (old)"), "{old}");
    // The full preview says it too.
    editor.handle_key(key(KeyCode::F(11)));
    assert!(
        title(&mut editor).contains("(old)"),
        "{}",
        title(&mut editor)
    );

    editor.report = report_with(true, &[]);
    assert!(
        !title(&mut editor).contains("(old)"),
        "a good report ends it"
    );
    // With no page at all, there is nothing to call old.
    let other = temp_file("oldpage2", "= Two\n");
    let mut fresh = open(&other);
    fresh.report = report_with(false, &["doc.typ:1:1: error: boom"]);
    assert!(!screen_rows(&mut fresh)[0].contains("(old)"));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
    fs::remove_dir_all(other.parent().unwrap()).unwrap();
}

#[test]
fn the_status_line_names_the_section_of_the_cursor() {
    let path = temp_file(
        "section",
        "intro\n= Notes\ntext\n== Sub part\nmore\n/*\n= not a heading\n*/\nafter\n",
    );
    let mut editor = open(&path);
    assert!(
        !status_row_at(&mut editor, 160).contains("="),
        "no heading above the cursor: {}",
        status_row_at(&mut editor, 160)
    );
    for (row, section) in [
        (1, "= Notes"),
        (2, "= Notes"),
        (3, "== Sub part"),
        (4, "== Sub part"),
        (8, "== Sub part"),
    ] {
        editor.textarea.move_cursor(CursorMove::Jump(row, 0));
        let status = status_row_at(&mut editor, 160);
        assert!(
            status.contains(&format!("{section}  ")),
            "row {row}: {status}"
        );
        assert!(
            status.trim_end().ends_with(&format!("{}:1", row + 1)),
            "{status}"
        );
    }
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_long_section_title_is_cut_and_a_narrow_screen_shows_none() {
    let long = format!("= {}", "very long ".repeat(12));
    let path = temp_file("sectionlong", &format!("{long}\ntext\n"));
    let mut editor = open(&path);
    editor.textarea.move_cursor(CursorMove::Jump(1, 0));
    let status = status_row_at(&mut editor, 160);
    assert!(status.contains("..."), "{status}");
    assert!(status.contains("F1 help"), "the hint is not cut: {status}");
    // At 100 columns there is no room: the hint stays whole and the section is not shown.
    let narrow = status_row_at(&mut editor, 100);
    assert!(!narrow.contains("very"), "{narrow}");
    assert!(narrow.contains("Esc back"), "{narrow}");
    assert!(narrow.trim_end().ends_with("2:1"), "{narrow}");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn ctrl_z_undoes_like_ctrl_u() {
    let path = temp_file("ctrlz", "text\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    assert_eq!(editor.textarea.lines(), ["Xtext"]);
    editor.handle_key(ctrl('z'));
    assert_eq!(editor.textarea.lines(), ["text"]);
    editor.handle_key(ctrl('r'));
    assert_eq!(editor.textarea.lines(), ["Xtext"], "Ctrl-R still redoes");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn alt_a_selects_all_the_text_and_ctrl_c_copies_it() {
    let path = temp_file("selectall", "one\ntwo\nthree\n");
    let mut editor = open(&path);
    editor.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::ALT));
    editor.handle_key(ctrl('c'));
    assert_eq!(editor.take_clipboard().as_deref(), Some("one\ntwo\nthree"));
    // Ctrl-A is still the start of the line.
    editor.textarea.move_cursor(CursorMove::Jump(1, 2));
    editor.handle_key(ctrl('a'));
    assert_eq!(editor.cursor_position(), (1, 0));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn ctrl_home_and_ctrl_end_go_to_the_start_and_the_end_of_the_file_and_shift_selects() {
    let path = temp_file("ctrlhome", "one\ntwo\nthree\n");
    let mut editor = open(&path);
    editor.textarea.move_cursor(CursorMove::Jump(1, 1));
    editor.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL));
    assert_eq!(editor.cursor_position(), (2, 5), "the end of the last line");
    editor.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::CONTROL));
    assert_eq!(editor.cursor_position(), (0, 0));
    // With Shift the keys select.
    editor.textarea.move_cursor(CursorMove::Jump(1, 1));
    editor.handle_key(KeyEvent::new(
        KeyCode::End,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    editor.handle_key(ctrl('c'));
    assert_eq!(editor.take_clipboard().as_deref(), Some("wo\nthree"));
    editor.textarea.move_cursor(CursorMove::Jump(1, 1));
    editor.handle_key(KeyEvent::new(
        KeyCode::Home,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    editor.handle_key(ctrl('c'));
    assert_eq!(editor.take_clipboard().as_deref(), Some("one\nt"));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn alt_g_goes_to_a_line_with_an_optional_column() {
    let path = temp_file("gotoline", "one\ntwo is here\nthree\nfour\n");
    let mut editor = open(&path);
    let alt_g = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::ALT);
    let type_text = |editor: &mut Editor, text: &str| {
        for letter in text.chars() {
            editor.handle_key(key(KeyCode::Char(letter)));
        }
    };
    editor.handle_key(alt_g);
    assert!(
        status_row(&mut editor).starts_with("Line:"),
        "{}",
        status_row(&mut editor)
    );
    type_text(&mut editor, "3");
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(editor.cursor_position(), (2, 0));
    assert!(matches!(editor.mode, Mode::Edit));
    // With a column.
    editor.handle_key(alt_g);
    type_text(&mut editor, "2:5");
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(editor.cursor_position(), (1, 4));
    // Beyond the end goes to the last line, and a column beyond the end to the end of the line.
    editor.handle_key(alt_g);
    type_text(&mut editor, "99:99");
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(editor.cursor_position(), (3, 4));
    assert!(editor.message.contains("last line"), "{}", editor.message);
    // Letters are not typed. A bad text keeps the prompt open with a message.
    editor.handle_key(alt_g);
    type_text(&mut editor, "ab1:");
    editor.handle_key(key(KeyCode::Enter));
    assert!(
        matches!(editor.mode, Mode::Line(_)),
        "a bad text keeps the prompt open"
    );
    assert!(editor.message.contains("line number"), "{}", editor.message);
    editor.handle_key(key(KeyCode::Esc));
    // An empty prompt with Enter closes it.
    editor.handle_key(alt_g);
    editor.handle_key(key(KeyCode::Enter));
    assert!(matches!(editor.mode, Mode::Edit));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_line_prompt_closes_with_esc_and_takes_a_paste_and_does_not_change_the_text() {
    let path = temp_file("gotoline2", "one\ntwo\nthree\n");
    let mut editor = open(&path);
    editor.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::ALT));
    editor.paste("2\n");
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(
        editor.cursor_position(),
        (1, 0),
        "the paste went into the prompt"
    );
    assert_eq!(editor.textarea.lines(), ["one", "two", "three"]);
    assert!(!editor.dirty);
    editor.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::ALT));
    editor.handle_key(key(KeyCode::Esc));
    assert!(matches!(editor.mode, Mode::Edit));
    assert_eq!(editor.cursor_position(), (1, 0), "Esc keeps the cursor");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn esc_in_a_conflict_warns_then_closes_and_keeps_the_disk_version() {
    let path = temp_file("escconflict", "text\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    fs::write(&path, "outside\n").unwrap();

    assert!(matches!(editor.handle_key(key(KeyCode::Esc)), Action::Stay));
    assert!(
        editor.message.contains("changed on disk"),
        "{}",
        editor.message
    );
    assert!(editor.message.contains("Esc again"), "{}", editor.message);
    assert!(matches!(
        editor.handle_key(key(KeyCode::Esc)),
        Action::Close
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), "outside\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn esc_after_a_failed_write_warns_then_closes() {
    use std::os::unix::fs::PermissionsExt;
    let path = temp_file("escreadonly", "text\n");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));

    assert!(matches!(editor.handle_key(key(KeyCode::Esc)), Action::Stay));
    assert!(editor.message.contains("Save failed"), "{}", editor.message);
    assert!(matches!(
        editor.handle_key(key(KeyCode::Esc)),
        Action::Close
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), "text\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn another_key_cancels_the_close_warning() {
    let path = temp_file("cancel", "text\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    fs::write(&path, "outside\n").unwrap();
    editor.handle_key(key(KeyCode::Esc));
    editor.handle_key(key(KeyCode::Char('Y')));
    assert!(matches!(editor.handle_key(key(KeyCode::Esc)), Action::Stay));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn ctrl_b_on_an_unchanged_buffer_does_not_write_the_file() {
    let path = temp_file("nowrite", "");
    fs::write(&path, "= A\r\nb\r\n").unwrap();
    let mut editor = open(&path);
    editor.handle_key(ctrl('b'));
    assert!(editor.job.is_some(), "Ctrl-B must still compile");
    assert_eq!(fs::read(&path).unwrap(), b"= A\r\nb\r\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn ctrl_b_does_not_overwrite_a_change_made_by_another_program() {
    let path = temp_file("outside", "= Mine\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    fs::write(&path, "= Changed outside\n").unwrap();

    editor.handle_key(ctrl('b'));
    assert_eq!(fs::read_to_string(&path).unwrap(), "= Changed outside\n");
    assert!(
        editor.message.contains("changed on disk"),
        "{}",
        editor.message
    );
    assert!(
        editor.job.is_none(),
        "no compile of a file that was not saved"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_autosave_does_not_overwrite_a_change_made_by_another_program() {
    let path = temp_file("outside-auto", "= Mine\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    fs::write(&path, "= Changed outside\n").unwrap();

    editor.tick(Instant::now() + Duration::from_millis(400));
    assert_eq!(fs::read_to_string(&path).unwrap(), "= Changed outside\n");
    assert!(editor.dirty);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn ctrl_s_overwrites_a_change_made_by_another_program() {
    let path = temp_file("overwrite", "= Mine\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    fs::write(&path, "= Changed outside\n").unwrap();
    editor.handle_key(ctrl('b')); // finds the change and refuses

    editor.handle_key(ctrl('s'));
    assert_eq!(fs::read_to_string(&path).unwrap(), "X= Mine\n");
    assert!(!editor.dirty);
    // After the overwrite, the next save works without a warning.
    editor.handle_key(key(KeyCode::Char('Y')));
    editor.handle_key(ctrl('b'));
    assert_eq!(fs::read_to_string(&path).unwrap(), "XY= Mine\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_autosave_waits_for_a_quiet_300_ms() {
    let t0 = Instant::now();
    assert!(!debounce_done(None, t0));
    assert!(!debounce_done(Some(t0), t0));
    assert!(!debounce_done(Some(t0), t0 + Duration::from_millis(299)));
    assert!(debounce_done(Some(t0), t0 + DEBOUNCE));
    // A clock that runs back is not a pause.
    assert!(!debounce_done(Some(t0 + DEBOUNCE), t0));
}

#[test]
fn typing_alone_does_not_save_or_compile() {
    let path = temp_file("typing", "= Title\n");
    let mut editor = open(&path);
    for c in "abcdefghij".chars() {
        editor.handle_key(key(KeyCode::Char(c)));
    }
    assert!(!editor.tick(Instant::now()));
    assert!(editor.job.is_none());
    assert!(editor.dirty);
    assert_eq!(fs::read_to_string(&path).unwrap(), "= Title\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_pause_after_ten_keys_saves_and_compiles_once() {
    let path = temp_file("pause", "= Title\n");
    let mut editor = open(&path);
    for c in "abcdefghij".chars() {
        editor.handle_key(key(KeyCode::Char(c)));
    }
    let later = Instant::now() + Duration::from_millis(400);
    assert!(editor.tick(later));
    assert!(!editor.dirty);
    assert!(editor.job.is_some());
    assert_eq!(fs::read_to_string(&path).unwrap(), "abcdefghij= Title\n");

    wait_for_report(&mut editor);
    assert!(editor.report.as_ref().unwrap().ok);
    assert!(editor.preview.has_page());
    // The same burst of keys does not start a second compile.
    assert!(!editor.tick(later));
    assert!(editor.job.is_none());
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_manual_save_cancels_the_autosave() {
    let path = temp_file("cancel-auto", "= Title\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    editor.handle_key(ctrl('s'));
    assert!(!editor.tick(Instant::now() + Duration::from_millis(400)));
    assert!(editor.job.is_none());
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// Draws the editor on a 100 by 24 screen and returns all the text on it.
fn screen_text(editor: &mut Editor) -> String {
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| editor.draw(frame)).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn the_title_shows_the_path_relative_to_the_root() {
    let path = temp_file("title", "text\n");
    let mut editor = open(&path);
    let text = screen_text(&mut editor);
    assert!(text.contains("doc.typ"), "{text}");
    assert!(
        !text.contains(&path.parent().unwrap().display().to_string()),
        "{text}"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_long_line_wraps_on_screen_but_stays_one_line_in_the_file() {
    let long = format!("{}END", "word ".repeat(30));
    let path = temp_file("wrap", &format!("{long}\n"));
    let mut editor = open(&path);
    // The editor pane is 48 cells wide inside its border, so the line needs 4 screen rows.
    assert!(
        screen_text(&mut editor).contains("END"),
        "the end of the line is not on screen"
    );

    editor.handle_key(key(KeyCode::Char('X')));
    editor.handle_key(ctrl('s'));
    assert_eq!(fs::read_to_string(&path).unwrap(), format!("X{long}\n"));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// The rows of the screen as text, one string for each row. The editor draws on 100 columns and 24 rows.
fn screen_rows(editor: &mut Editor) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| editor.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..24)
        .map(|row| {
            (0..100)
                .map(|column| buffer[(column, row)].symbol())
                .collect()
        })
        .collect()
}

/// The first 50 columns of the rows that belong to the text area: the left half, without the border.
fn text_area_rows(editor: &mut Editor) -> Vec<String> {
    let rows = screen_rows(editor);
    // Skip the top border and the status line.
    rows[1..rows.len() - 1]
        .iter()
        .map(|row| row.chars().skip(1).take(48).collect())
        .collect()
}

#[test]
fn each_line_shows_its_number_at_the_left_edge() {
    let path = temp_file("numbers", "alpha\nbeta\ngamma\n");
    let mut editor = open(&path);
    let rows = text_area_rows(&mut editor);
    assert!(
        rows[0].trim_start().starts_with("1 alpha"),
        "{:?}",
        &rows[..4]
    );
    assert!(
        rows[1].trim_start().starts_with("2 beta"),
        "{:?}",
        &rows[..4]
    );
    assert!(
        rows[2].trim_start().starts_with("3 gamma"),
        "{:?}",
        &rows[..4]
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_line_that_wraps_shows_its_number_on_the_first_row_only() {
    let long = format!("{}end", "word ".repeat(30));
    let path = temp_file("wrapnumbers", &format!("{long}\nnext\n"));
    let mut editor = open(&path);
    let rows = text_area_rows(&mut editor);
    let with_number: Vec<_> = rows
        .iter()
        .filter(|row| row.chars().any(|c| c.is_ascii_digit()))
        .collect();
    assert_eq!(
        with_number.len(),
        2,
        "one number for each line, not for each row: {:?}",
        &rows[..8]
    );
    assert!(
        rows[0].trim_start().starts_with("1 word"),
        "{:?}",
        &rows[..8]
    );
    let wrapped = rows
        .iter()
        .filter(|row| row.contains("word") || row.contains("end"))
        .count();
    assert!(
        wrapped >= 3,
        "the long line must wrap on several rows: {:?}",
        &rows[..8]
    );
    let next = rows.iter().find(|row| row.contains("next")).unwrap();
    assert!(next.trim_start().starts_with("2 next"), "{next:?}");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_line_numbers_are_dim() {
    let path = temp_file("dimnumbers", "alpha\n");
    let mut editor = open(&path);
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| editor.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    let number = (1..8)
        .map(|column| &buffer[(column, 1)])
        .find(|cell| cell.symbol() == "1")
        .unwrap();
    assert!(
        number.modifier.contains(ratatui::style::Modifier::DIM),
        "the number must be dim"
    );
    let letter = (1..12)
        .map(|column| &buffer[(column, 1)])
        .find(|cell| cell.symbol() == "a")
        .unwrap();
    assert!(
        !letter.modifier.contains(ratatui::style::Modifier::DIM),
        "the text must not be dim"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// The status line of the editor: the last row of the screen.
fn status_row(editor: &mut Editor) -> String {
    screen_rows(editor).pop().unwrap()
}

/// The status line on a screen that is `width` columns wide.
fn status_row_at(editor: &mut Editor, width: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, 20)).unwrap();
    terminal.draw(|frame| editor.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..width)
        .map(|column| buffer[(column, 19)].symbol())
        .collect()
}

#[test]
fn the_status_line_shows_the_word_count_and_it_follows_the_edits() {
    let path = temp_file(
        "wordcount",
        "= Title\nHello brave world\n#set text(size: 11pt)\n",
    );
    let mut editor = open(&path);
    assert!(status_row(&mut editor).contains("4 words"));
    editor.paste("one two\n");
    assert!(status_row(&mut editor).contains("6 words"));
    editor.handle_key(key(KeyCode::Char('x')));
    assert!(status_row(&mut editor).contains("7 words"));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_status_line_shows_the_cursor_position_at_its_right_end() {
    let path = temp_file("position", "line one\nline two\nline three\n");
    let mut editor = open(&path);
    assert!(
        status_row(&mut editor).trim_end().ends_with("1:1"),
        "{:?}",
        status_row(&mut editor)
    );

    editor.handle_key(key(KeyCode::Down));
    editor.handle_key(key(KeyCode::Down));
    for _ in 0..4 {
        editor.handle_key(key(KeyCode::Right));
    }
    assert!(
        status_row(&mut editor).trim_end().ends_with("3:5"),
        "{:?}",
        status_row(&mut editor)
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_position_stays_when_the_status_line_shows_a_message() {
    let path = temp_file("positionmessage", "text\n");
    let mut editor = open(&path);
    editor.message = "Saved".into();
    let row = status_row(&mut editor);
    assert!(
        row.starts_with("Saved") && row.trim_end().ends_with("1:1"),
        "{row:?}"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_column_counts_characters_like_typst_does() {
    let path = temp_file("positionunicode", "é #nope()\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Right)); // over the é
    assert!(
        status_row(&mut editor).trim_end().ends_with("1:2"),
        "{:?}",
        status_row(&mut editor)
    );

    // Ctrl-G moves to the position that Typst names, and the status line shows the same position.
    editor.handle_key(ctrl('b'));
    wait_for_report(&mut editor);
    editor.handle_key(ctrl('g'));
    assert!(
        status_row(&mut editor).trim_end().ends_with("1:3"),
        "{:?}",
        status_row(&mut editor)
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_position_does_not_cut_the_hint_on_a_wide_screen_and_wins_on_a_narrow_one() {
    let path = temp_file("positionnarrow", "text\n");
    let mut editor = open(&path);
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
    terminal.draw(|frame| editor.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    let row: String = (0..40)
        .map(|column| buffer[(column, 11)].symbol())
        .collect();
    assert!(row.trim_end().ends_with("1:1"), "{row:?}");
    assert!(row.starts_with("F1 help"), "{row:?}");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn tab_at_the_start_of_a_line_inserts_exactly_two_spaces() {
    let path = temp_file("tab", "text\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Tab));
    assert_eq!(editor.textarea.lines()[0], "  text");
    editor.handle_key(key(KeyCode::Tab));
    assert_eq!(
        editor.textarea.lines()[0],
        "    text",
        "a second Tab adds two more spaces"
    );
    assert!(editor.dirty);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn tab_in_the_middle_of_a_line_goes_to_the_next_stop_of_two_columns() {
    let path = temp_file("tabmiddle", "ab\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Right)); // column 1
    editor.handle_key(key(KeyCode::Tab));
    assert_eq!(
        editor.textarea.lines()[0],
        "a b",
        "one space to reach the stop at column 2"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn tab_inserts_spaces_and_never_a_tab_character_in_the_file() {
    let path = temp_file("tabsoft", "text\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Tab));
    editor.handle_key(ctrl('s'));
    let saved = fs::read_to_string(&path).unwrap();
    assert_eq!(saved, "  text\n");
    assert!(!saved.contains('\t'));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn tab_characters_that_are_already_in_the_file_stay_on_save() {
    let path = temp_file("tabkeep", "\tindented\n\tmore\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    editor.handle_key(ctrl('s'));
    assert_eq!(fs::read_to_string(&path).unwrap(), "X\tindented\n\tmore\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_indent_is_found_from_the_file() {
    use super::{Indent, detect_indent};
    assert_eq!(detect_indent(""), Indent::Spaces(2));
    assert_eq!(detect_indent("a\nb\n"), Indent::Spaces(2));
    assert_eq!(detect_indent("a {\n  b\n    c\n  d\n"), Indent::Spaces(2));
    assert_eq!(
        detect_indent("a {\n    b\n        c\n    d\n"),
        Indent::Spaces(4)
    );
    assert_eq!(detect_indent("a {\n\tb\n\t\tc\n"), Indent::Tab);
    assert_eq!(
        detect_indent("a {\n\tb\n  c\n    d\n"),
        Indent::Spaces(2),
        "spaces win when more lines use them"
    );
    assert_eq!(detect_indent("a\n   b\n"), Indent::Spaces(3));
    assert_eq!(
        detect_indent("a\n     b\n"),
        Indent::Spaces(2),
        "an odd step is not a style"
    );
}

#[test]
fn tab_shift_tab_and_enter_follow_the_indent_of_the_file() {
    let path = temp_file("indent4", "fn {\n    a\n        b\n}\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Tab));
    assert_eq!(editor.textarea.lines()[0], "    fn {");
    editor.handle_key(key(KeyCode::BackTab));
    assert_eq!(editor.textarea.lines()[0], "fn {");
    editor.handle_key(key(KeyCode::End));
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(
        editor.textarea.lines()[1],
        "    ",
        "one level of 4 after an opening bracket"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();

    let path = temp_file("indenttab", "fn {\n\ta\n\t\tb\n}\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Tab));
    assert_eq!(editor.textarea.lines()[0], "\tfn {");
    editor.handle_key(key(KeyCode::BackTab));
    assert_eq!(editor.textarea.lines()[0], "fn {");
    editor.handle_key(key(KeyCode::End));
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(
        editor.textarea.lines()[1],
        "\t",
        "a tab after an opening bracket"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// Opens `doc.typ`, which includes `part.typ`, and waits for the first compile to end.
fn editor_with_part(name: &str) -> (PathBuf, Editor) {
    let path = temp_file(name, "#include \"part.typ\"\n");
    fs::write(path.with_file_name("part.typ"), "= Part\n").unwrap();
    let mut editor = open(&path);
    editor.compile_now();
    wait_for_idle(&mut editor);
    (path, editor)
}

/// Gives `file` a modification time `seconds` in the future, so a change shows at once.
fn touch_later(file: &Path, seconds: u64) {
    let time = SystemTime::now() + Duration::from_secs(seconds);
    fs::File::options()
        .write(true)
        .open(file)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

#[test]
fn a_change_of_an_included_file_starts_a_compile_and_names_the_file() {
    let (path, mut editor) = editor_with_part("depschange");
    let part = path.with_file_name("part.typ");
    assert!(
        editor.deps.iter().any(|(file, _)| *file == part),
        "{:?}",
        editor.deps
    );
    let later = Instant::now() + Duration::from_secs(5);
    assert!(!editor.tick(later), "nothing changed, so nothing happens");
    assert!(!editor.compiling());
    fs::write(&part, "= Changed\n").unwrap();
    touch_later(&part, 30);
    assert!(editor.tick(later + Duration::from_secs(2)));
    assert!(editor.compiling());
    assert_eq!(editor.message, "part.typ changed");
    wait_for_idle(&mut editor);
    let again = Instant::now() + Duration::from_secs(20);
    assert!(
        !editor.tick(again) && !editor.compiling(),
        "the new time is the new normal"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_file_that_appears_after_a_failed_compile_starts_a_compile() {
    let path = temp_file("depsmissing", "#include \"later.typ\"\n");
    let mut editor = open(&path);
    editor.compile_now();
    wait_for_idle(&mut editor);
    assert!(editor.report.as_ref().is_some_and(|report| !report.ok));
    fs::write(path.with_file_name("later.typ"), "= Here\n").unwrap();
    assert!(editor.tick(Instant::now() + Duration::from_secs(2)));
    assert!(editor.compiling());
    assert_eq!(editor.message, "later.typ changed");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_dependency_watch_waits_while_the_user_types_or_the_live_compile_is_off() {
    let (path, mut editor) = editor_with_part("depswait");
    let part = path.with_file_name("part.typ");
    fs::write(&part, "= Changed\n").unwrap();
    touch_later(&part, 30);
    editor.paused = true;
    assert!(!editor.tick(Instant::now() + Duration::from_secs(2)));
    assert!(!editor.compiling(), "F5 turned the live compile off");
    editor.paused = false;
    editor.dirty = true;
    assert!(!editor.tick(Instant::now() + Duration::from_secs(4)));
    assert!(!editor.compiling(), "the buffer has edits");
    editor.dirty = false;
    assert!(editor.tick(Instant::now() + Duration::from_secs(6)));
    assert!(editor.compiling());
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// Opens a file with `text`, searches for `term` with Ctrl-F, and opens the prompt `Replace with:` with Alt-S.
fn replace_prompt(name: &str, text: &str, term: &str) -> (PathBuf, Editor) {
    let path = temp_file(name, text);
    let mut editor = open(&path);
    editor.handle_key(ctrl('f'));
    for letter in term.chars() {
        editor.handle_key(key(KeyCode::Char(letter)));
    }
    editor.handle_key(alt(KeyCode::Char('s')));
    assert!(matches!(editor.mode, Mode::Replace(_)));
    (path, editor)
}

fn type_text(editor: &mut Editor, text: &str) {
    for letter in text.chars() {
        editor.handle_key(key(KeyCode::Char(letter)));
    }
}

#[test]
fn enter_replaces_one_match_and_goes_to_the_next() {
    let (path, mut editor) = replace_prompt("replace1", "cat and Cat\ncat\n", "cat");
    type_text(&mut editor, "dog");
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(editor.textarea.lines(), ["dog and Cat", "cat"]);
    assert_eq!(editor.message, "Replaced. 2 left");
    assert_eq!(
        editor.cursor_position(),
        (0, 8),
        "the cursor is on the next match"
    );
    editor.handle_key(key(KeyCode::Enter));
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(editor.textarea.lines(), ["dog and dog", "dog"]);
    assert_eq!(editor.message, "Replaced. No more matches");
    editor.handle_key(key(KeyCode::Enter));
    assert!(editor.message.starts_with("No match for cat"));
    assert!(editor.dirty);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn alt_a_replaces_all_matches_in_one_undo_step() {
    let (path, mut editor) = replace_prompt("replaceall", "a cat\n\ncat cat\n", "cat");
    type_text(&mut editor, "dog");
    editor.handle_key(alt(KeyCode::Char('a')));
    assert_eq!(editor.textarea.lines(), ["a dog", "", "dog dog"]);
    assert_eq!(editor.message, "Replaced 3 matches");
    editor.handle_key(key(KeyCode::Esc));
    assert!(matches!(editor.mode, Mode::Edit));
    editor.handle_key(ctrl('u'));
    assert_eq!(
        editor.textarea.lines(),
        ["a cat", "", "cat cat"],
        "one undo takes all back"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn one_replace_is_one_undo_step() {
    let (path, mut editor) = replace_prompt("replaceundo", "cat cat\n", "cat");
    type_text(&mut editor, "tiger");
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(editor.textarea.lines(), ["tiger cat"]);
    editor.handle_key(key(KeyCode::Esc));
    editor.handle_key(ctrl('u'));
    assert_eq!(editor.textarea.lines(), ["cat cat"]);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_replacement_is_plain_text_and_regex_groups_work_in_regex_mode() {
    let (path, mut editor) = replace_prompt("replaceplain", "price (1)\n", "(1)");
    type_text(&mut editor, "$1 & \\n");
    editor.handle_key(alt(KeyCode::Char('a')));
    assert_eq!(
        editor.textarea.lines(),
        ["price $1 & \\n"],
        "no expansion in plain mode"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();

    let path = temp_file("replaceregex", "ab-12 cd-34\n");
    let mut editor = open(&path);
    editor.handle_key(ctrl('f'));
    editor.handle_key(alt(KeyCode::Char('r')));
    type_text(&mut editor, "([a-z]+)-([0-9]+)");
    editor.handle_key(alt(KeyCode::Char('s')));
    type_text(&mut editor, "$2:$1");
    editor.handle_key(alt(KeyCode::Char('a')));
    assert_eq!(editor.textarea.lines(), ["12:ab 34:cd"]);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn alt_s_without_a_search_says_so_and_a_search_without_a_match_replaces_nothing() {
    let path = temp_file("replacenone", "text\n");
    let mut editor = open(&path);
    editor.handle_key(alt(KeyCode::Char('s')));
    assert!(matches!(editor.mode, Mode::Edit));
    assert!(
        editor.message.starts_with("Search first"),
        "{}",
        editor.message
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();

    let (path, mut editor) = replace_prompt("replacenomatch", "text\n", "zzz");
    editor.handle_key(alt(KeyCode::Char('a')));
    assert!(
        editor.message.starts_with("No match for zzz"),
        "{}",
        editor.message
    );
    assert_eq!(editor.textarea.lines(), ["text"]);
    assert!(!editor.dirty);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_replace_prompt_takes_a_paste_and_remembers_the_text_and_esc_leaves_the_text_alone() {
    let (path, mut editor) = replace_prompt("replacepaste", "cat\n", "cat");
    editor.paste("dog\n");
    editor.handle_key(key(KeyCode::Esc));
    assert_eq!(editor.textarea.lines(), ["cat"]);
    editor.handle_key(ctrl('f'));
    editor.handle_key(alt(KeyCode::Char('s')));
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(
        editor.textarea.lines(),
        ["dog"],
        "the prompt started with the last replacement"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_replace_prompt_shows_its_text_in_a_narrow_window_and_keeps_it_after_the_window_grows() {
    let (path, mut editor) = replace_prompt("replacenarrow", "cat\n", "cat");
    let last_row = |editor: &mut Editor, width: u16| {
        let mut terminal = Terminal::new(TestBackend::new(width, 20)).unwrap();
        terminal.draw(|frame| editor.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..width)
            .map(|x| buffer[(x, 19)].symbol())
            .collect::<String>()
    };
    last_row(&mut editor, 56);
    type_text(&mut editor, "dog");
    assert!(last_row(&mut editor, 56).contains("Replace with: dog"));
    assert!(last_row(&mut editor, 100).contains("Replace with: dog"));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

const OUTLINE_TEXT: &str = "= One\ntext\n== Two A\n// = not a heading\n```\n= in raw\n```\n=== Deep one\n= Three\n$\n= in math\n$\n";

#[test]
fn the_outline_lists_the_headings_by_level_and_skips_comments_raw_text_and_math() {
    let path = temp_file("outlinelist", OUTLINE_TEXT);
    let editor = open(&path);
    let headings = editor.headings();
    assert_eq!(
        headings,
        [
            (0, "One".to_string()),
            (2, "  Two A".to_string()),
            (7, "    Deep one".to_string()),
            (8, "Three".to_string()),
        ]
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn f4_opens_the_outline_on_the_section_of_the_cursor_and_enter_jumps() {
    let path = temp_file("outlinejump", OUTLINE_TEXT);
    let mut editor = open(&path);
    editor.set_cursor_position((3, 0)); // after "== Two A"
    editor.handle_key(key(KeyCode::F(4)));
    assert!(matches!(editor.mode, Mode::Outline(_)));
    editor.handle_key(key(KeyCode::Enter));
    assert!(matches!(editor.mode, Mode::Edit));
    assert_eq!(
        editor.cursor_position(),
        (2, 0),
        "the section of the cursor was selected"
    );
    editor.handle_key(key(KeyCode::F(4)));
    type_text(&mut editor, "thr");
    editor.handle_key(key(KeyCode::Enter));
    assert_eq!(editor.cursor_position(), (8, 0), "the filter found Three");
    assert!(!editor.dirty, "the outline never changes the text");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_outline_closes_with_esc_and_f4_and_a_file_without_headings_says_so() {
    let path = temp_file("outlineclose", "= One\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::F(4)));
    editor.handle_key(key(KeyCode::Esc));
    assert!(matches!(editor.mode, Mode::Edit));
    editor.handle_key(key(KeyCode::F(4)));
    editor.handle_key(key(KeyCode::F(4)));
    assert!(matches!(editor.mode, Mode::Edit));
    editor.handle_key(key(KeyCode::F(4)));
    editor.paste("x");
    assert!(!editor.dirty, "a paste goes to the filter");
    assert!(matches!(editor.mode, Mode::Outline(_)));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();

    let path = temp_file("outlinenone", "just text\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::F(4)));
    assert!(matches!(editor.mode, Mode::Edit));
    assert!(
        editor.message.starts_with("No headings"),
        "{}",
        editor.message
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_string_near_the_cursor_is_the_one_that_holds_it_or_else_the_first() {
    use super::string_near;
    let line = r#"#include "a.typ" and #image("b.png", alt: "x \" y")"#;
    assert_eq!(string_near(line, 12).as_deref(), Some("a.typ"));
    assert_eq!(
        string_near(line, 8).as_deref(),
        Some("a.typ"),
        "on the opening quote"
    );
    assert_eq!(string_near(line, 31).as_deref(), Some("b.png"));
    assert_eq!(
        string_near(line, 0).as_deref(),
        Some("a.typ"),
        "else the first string"
    );
    assert_eq!(
        string_near(line, 46).as_deref(),
        Some("x \\\" y"),
        "an escaped quote stays inside"
    );
    assert_eq!(string_near("no strings", 3), None);
    assert_eq!(
        string_near("\"open", 2),
        None,
        "a string that never ends is none"
    );
}

/// Makes `root/main.typ` with `body`, `root/chapters/two.typ`, `root/img.png`, and `outside.typ` beside root.
fn include_project(name: &str, body: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("lazytypst-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let root = base.join("root");
    fs::create_dir_all(root.join("chapters")).unwrap();
    fs::write(root.join("main.typ"), body).unwrap();
    fs::write(
        root.join("chapters/two.typ"),
        "= Two\n#include \"three.typ\"\n",
    )
    .unwrap();
    fs::write(root.join("chapters/three.typ"), "= Three\n").unwrap();
    fs::write(root.join("img.png"), "x").unwrap();
    fs::write(base.join("outside.typ"), "x").unwrap();
    (base, root)
}

fn open_in(root: &Path, file: &str) -> Editor {
    let path = root.join(file);
    Editor::open(path, root.to_path_buf(), None, Picker::halfblocks()).unwrap()
}

#[test]
fn alt_enter_and_ctrl_bracket_go_to_the_included_file_and_save_first() {
    let body = "#include \"chapters/two.typ\"\n#import \"chapters/three.typ\": x\n";
    let (base, root) = include_project("gofile", body);
    let mut editor = open_in(&root, "main.typ");
    editor.handle_key(key(KeyCode::Char('Z')));
    let action = editor.handle_key(alt(KeyCode::Enter));
    assert!(
        matches!(&action, Action::Goto { file, line: 1, column: 1 } if file == Path::new("chapters/two.typ")),
        "{action:?}"
    );
    assert_eq!(
        fs::read_to_string(root.join("main.typ")).unwrap(),
        format!("Z{body}"),
        "saved first"
    );
    // The import on line 2: the cursor is on the first string of the line.
    editor.handle_key(key(KeyCode::Down));
    let action = editor.handle_key(ctrl(']'));
    assert!(
        matches!(&action, Action::Goto { file, .. } if file == Path::new("chapters/three.typ")),
        "{action:?}"
    );
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn a_name_is_read_from_the_folder_of_the_file_and_a_slash_from_the_root() {
    let (base, root) = include_project("gorelative", "x\n");
    let mut editor = open_in(&root, "chapters/two.typ");
    editor.handle_key(key(KeyCode::Down));
    assert!(
        matches!(editor.handle_key(alt(KeyCode::Enter)), Action::Goto { file, .. } if file == Path::new("chapters/three.typ"))
    );
    let name = |text: &str| resolve_file(&root, &root.join("chapters/two.typ"), text);
    assert_eq!(name("/main.typ"), Ok(PathBuf::from("main.typ")));
    assert_eq!(name("../main.typ"), Ok(PathBuf::from("main.typ")));
    assert_eq!(name("./three.typ"), Ok(PathBuf::from("chapters/three.typ")));
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn a_missing_file_a_non_typ_file_a_package_and_a_path_outside_the_project_are_only_named() {
    let (base, root) = include_project("gobad", "x\n");
    let open = root.join("main.typ");
    assert_eq!(
        resolve_file(&root, &open, "nope.typ"),
        Err("nope.typ is not a file.".into())
    );
    assert!(
        resolve_file(&root, &open, "img.png")
            .unwrap_err()
            .contains("not a .typ file")
    );
    assert!(
        resolve_file(&root, &open, "@preview/cetz:0.3.0")
            .unwrap_err()
            .contains("package")
    );
    assert!(
        resolve_file(&root, &open, "../outside.typ")
            .unwrap_err()
            .contains("outside the project")
    );
    assert!(
        resolve_file(&root, &open, "../../etc/passwd")
            .unwrap_err()
            .contains("outside the project")
    );
    std::os::unix::fs::symlink(base.join("outside.typ"), root.join("link.typ")).unwrap();
    assert!(
        resolve_file(&root, &open, "link.typ")
            .unwrap_err()
            .contains("outside the project"),
        "a link that leaves"
    );
    // In the editor the message shows and the text stays.
    fs::write(&open, "#include \"nope.typ\"\n").unwrap();
    let mut editor = open_in(&root, "main.typ");
    assert!(matches!(
        editor.handle_key(alt(KeyCode::Enter)),
        Action::Stay
    ));
    assert_eq!(editor.message, "nope.typ is not a file.");
    fs::write(&open, "no string\n").unwrap();
    let mut editor = open_in(&root, "main.typ");
    editor.handle_key(ctrl(']'));
    assert!(
        editor.message.starts_with("No string"),
        "{}",
        editor.message
    );
    assert!(!editor.dirty);
    fs::remove_dir_all(base).unwrap();
}

/// The screen of the editor as rows of text, in a window of `width` by `height`.
fn rows_sized(editor: &mut Editor, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| editor.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
        .collect()
}

fn row_with(rows: &[String], text: &str) -> Option<usize> {
    rows.iter().position(|row| row.contains(text))
}

#[test]
fn a_wide_window_starts_side_by_side_and_a_narrow_one_starts_stacked() {
    let path = temp_file("layoutauto", "text\n");
    let mut editor = open(&path);
    let wide = rows_sized(&mut editor, 120, 30);
    let (editor_row, preview_row) = (
        row_with(&wide, "doc.typ").unwrap(),
        row_with(&wide, "Preview").unwrap(),
    );
    assert_eq!(
        editor_row, preview_row,
        "side by side: the two titles share a row"
    );
    let narrow = rows_sized(&mut editor, 80, 30);
    let (editor_row, preview_row) = (
        row_with(&narrow, "doc.typ").unwrap(),
        row_with(&narrow, "Preview").unwrap(),
    );
    assert!(
        preview_row > editor_row,
        "stacked: the preview is below the editor"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn f10_cycles_the_layouts_and_the_editor_only_layout_hides_the_preview() {
    let path = temp_file("layoutcycle", "text\n");
    let mut editor = open(&path);
    rows_sized(&mut editor, 120, 30); // the draw tells the editor the width
    editor.handle_key(key(KeyCode::F(10)));
    assert_eq!(
        editor.chosen_arrangement(),
        Some(Arrangement::Stacked),
        "from side by side"
    );
    assert_eq!(editor.message, "Layout: stacked");
    let rows = rows_sized(&mut editor, 120, 30);
    assert!(row_with(&rows, "Preview").unwrap() > row_with(&rows, "doc.typ").unwrap());
    editor.handle_key(key(KeyCode::F(10)));
    assert_eq!(editor.chosen_arrangement(), Some(Arrangement::EditorOnly));
    let rows = rows_sized(&mut editor, 120, 30);
    assert!(row_with(&rows, "Preview").is_none(), "no preview");
    assert!(
        row_with(&rows, "Compile").is_some(),
        "the compile pane stays"
    );
    editor.handle_key(key(KeyCode::F(10)));
    assert_eq!(editor.chosen_arrangement(), Some(Arrangement::Side));
    // A chosen layout does not follow the width any more.
    let rows = rows_sized(&mut editor, 80, 30);
    assert_eq!(row_with(&rows, "doc.typ"), row_with(&rows, "Preview"));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn every_layout_draws_in_small_and_tall_windows_without_a_panic() {
    let path = temp_file("layoutsizes", "text\n");
    let mut editor = open(&path);
    for arrangement in [
        Arrangement::Side,
        Arrangement::Stacked,
        Arrangement::EditorOnly,
    ] {
        editor.set_arrangement(Some(arrangement));
        for (width, height) in [(40, 10), (41, 11), (200, 8), (30, 60), (1, 1), (0, 0)] {
            rows_sized(&mut editor, width, height);
        }
    }
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_wait_before_a_compile_follows_the_time_of_the_last_one() {
    use super::compile_wait_after;
    let ms = Duration::from_millis;
    assert_eq!(compile_wait_after(ms(0)), ms(300));
    assert_eq!(
        compile_wait_after(ms(999)),
        ms(300),
        "a fast compile starts at once"
    );
    assert_eq!(compile_wait_after(ms(1000)), ms(2000));
    assert_eq!(compile_wait_after(ms(1400)), ms(2800));
    assert_eq!(
        compile_wait_after(ms(9000)),
        ms(3000),
        "not longer than 3 s"
    );
}

/// Gives the editor the report of a compile that took `seconds`.
fn after_compile_of(editor: &mut Editor, seconds: u64) {
    let mut report = Report::new(true, Vec::new());
    report.elapsed = Some(Duration::from_secs(seconds));
    editor.report = Some(report);
}

#[test]
fn after_a_slow_compile_the_save_comes_at_once_and_the_compile_waits() {
    let path = temp_file("slowwait", "text\n");
    let mut editor = open(&path);
    after_compile_of(&mut editor, 2);
    editor.handle_key(key(KeyCode::Char('X')));
    let start = Instant::now();
    assert!(editor.tick(start + Duration::from_millis(400)));
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "Xtext\n",
        "the save does not wait"
    );
    assert!(!editor.compiling(), "the compile waits");
    let pane = pane_rows(&mut editor).join("\n");
    assert!(pane.contains("next in 3.0 s"), "{pane}");
    assert!(
        !editor.tick(start + Duration::from_secs(2)),
        "still waiting"
    );
    assert!(!editor.compiling());
    assert!(editor.tick(start + Duration::from_secs(4)));
    assert!(editor.compiling(), "the wait is over");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_new_key_cancels_the_waiting_compile_and_ctrl_b_does_not_wait() {
    let path = temp_file("slowcancel", "text\n");
    let mut editor = open(&path);
    after_compile_of(&mut editor, 2);
    editor.handle_key(key(KeyCode::Char('X')));
    let start = Instant::now();
    editor.tick(start + Duration::from_millis(400));
    assert!(editor.compile_at.is_some());
    editor.handle_key(key(KeyCode::Char('Y')));
    assert!(editor.compile_at.is_none(), "a key cancels it");
    assert!(!editor.compiling());
    editor.handle_key(ctrl('b'));
    assert!(editor.compiling(), "Ctrl-B starts at once");
    assert!(editor.compile_at.is_none());
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_history_keeps_the_text_at_open_and_not_every_save_and_lists_it_with_f6() {
    let path = temp_file("historylist", "first\n");
    let state = path.parent().unwrap().join("state").join("history");
    let mut editor = open(&path);
    editor.set_history(Some(state.clone()));
    assert_eq!(
        history::list(&state, &path).len(),
        1,
        "the text at open is the first version"
    );
    editor.handle_key(key(KeyCode::Char('X')));
    editor.handle_key(ctrl('s'));
    assert_eq!(
        history::list(&state, &path).len(),
        1,
        "the next save is under 30 s later"
    );
    editor.handle_key(key(KeyCode::F(6)));
    assert!(matches!(editor.mode, Mode::History(_)));
    let rows = rows_sized(&mut editor, 100, 24).join("\n");
    assert!(
        rows.contains("History") && rows.contains("just now") && rows.contains("-1 +1 lines"),
        "{rows}"
    );
    editor.handle_key(key(KeyCode::Esc));
    assert!(matches!(editor.mode, Mode::Edit));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn enter_restores_an_older_version_as_one_edit_that_ctrl_z_takes_back() {
    let path = temp_file("historyrestore", "good text\nsecond line\n");
    let state = path.parent().unwrap().join("state").join("history");
    let long_ago = SystemTime::now() - Duration::from_secs(7200);
    history::record(&state, &path, "good text\nsecond line\n", long_ago, true).unwrap();
    fs::write(&path, "bad paste\n").unwrap();
    let mut editor = open(&path);
    editor.set_history(Some(state.clone()));
    editor.handle_key(key(KeyCode::F(6)));
    editor.handle_key(key(KeyCode::Down)); // the older version
    editor.handle_key(key(KeyCode::Enter));
    assert!(matches!(editor.mode, Mode::Edit));
    assert_eq!(editor.textarea.lines(), ["good text", "second line"]);
    assert!(editor.dirty, "a restore is an edit");
    assert!(editor.message.starts_with("Restored"), "{}", editor.message);
    editor.handle_key(ctrl('z'));
    assert_eq!(
        editor.textarea.lines(),
        ["bad paste"],
        "one Ctrl-Z takes it back"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn f6_without_versions_or_without_a_state_folder_says_so() {
    let path = temp_file("historynone", "text\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::F(6)));
    assert!(matches!(editor.mode, Mode::Edit));
    assert!(
        editor.message.contains("no state folder"),
        "{}",
        editor.message
    );
    let state = path.parent().unwrap().join("state").join("history");
    editor.history = Some(state);
    editor.handle_key(key(KeyCode::F(6)));
    assert!(
        editor.message.starts_with("No versions yet"),
        "{}",
        editor.message
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// A fake `hunspell`: every word is known, except the words that start with `zz`. Those get two suggestions.
fn fake_spell_program(dir: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join("hunspell");
    fs::write(
        &script,
        "#!/bin/sh\necho '@(#) fake'\nwhile read -r line; do\n  w=${line#^}\n  case \"$w\" in\n    zzq) echo \"# $w 0\";;\n    zz*) echo \"& $w 2 0: fixa, fixb\";;\n    *) echo '*';;\n  esac\n  echo\ndone\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    script.display().to_string()
}

/// An editor with the text, the fake program, and the spell check on and finished.
fn spell_editor(name: &str, text: &str) -> (PathBuf, Editor) {
    let path = temp_file(name, text);
    let mut editor = open(&path);
    editor.spelling.program = fake_spell_program(path.parent().unwrap());
    editor.set_spell(true, Some(path.parent().unwrap().join("words.txt")));
    wait_for_spell(&mut editor);
    (path, editor)
}

fn wait_for_spell(editor: &mut Editor) {
    let start = Instant::now();
    loop {
        editor.tick(Instant::now());
        if editor.spelling.checked_version == Some(editor.text_version)
            && editor.spelling.job.is_none()
        {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the check did not end"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_spell_check_underlines_unknown_words_of_the_prose_only() {
    let text = "hello zzworld okay\nsecond zzq line // zzcomment\n";
    let (path, mut editor) = spell_editor("spellmarks", text);
    assert_eq!(editor.spelling.marks[0], [(6, 13)]);
    assert_eq!(
        editor.spelling.marks[1],
        [(7, 10)],
        "the comment is not prose"
    );
    // The words are underlined on the screen.
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| editor.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    let underlined = |x: u16, y: u16| buffer[(x, y)].modifier.contains(Modifier::UNDERLINED);
    // The gutter is 3 columns and the border 1: the text starts at column 4.
    assert!(underlined(4 + 6, 1) && underlined(4 + 12, 1));
    assert!(!underlined(4, 1) && !underlined(4 + 13, 1));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn an_edit_hides_the_marks_until_the_next_check() {
    let (path, mut editor) = spell_editor("spellstale", "hello zzworld\n");
    assert!(!editor.spelling.marks_of(0, editor.text_version).is_empty());
    editor.handle_key(key(KeyCode::Char('X')));
    assert!(
        editor.spelling.marks_of(0, editor.text_version).is_empty(),
        "stale marks are hidden"
    );
    editor.tick(Instant::now() + Duration::from_secs(2)); // the save, and then the new check
    wait_for_spell(&mut editor);
    assert_eq!(
        editor.spelling.marks[0],
        [(7, 14)],
        "the new place of the word"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn alt_semicolon_goes_to_the_next_word_and_enter_replaces_it_as_one_edit() {
    let (path, mut editor) = spell_editor("spellfix", "hello zzworld and zzother\n");
    editor.handle_key(alt(KeyCode::Char(';')));
    assert!(matches!(editor.mode, Mode::Spell(_)));
    assert_eq!(editor.cursor_position(), (0, 6));
    editor.handle_key(key(KeyCode::Down)); // fixb
    editor.handle_key(key(KeyCode::Enter));
    assert!(matches!(editor.mode, Mode::Edit));
    assert_eq!(editor.textarea.lines(), ["hello fixb and zzother"]);
    editor.handle_key(ctrl('z'));
    assert_eq!(
        editor.textarea.lines(),
        ["hello zzworld and zzother"],
        "one Ctrl-Z takes it back"
    );
    editor.tick(Instant::now() + Duration::from_secs(2));
    wait_for_spell(&mut editor);
    editor.handle_key(alt(KeyCode::Char(';')));
    assert_eq!(
        editor.cursor_position(),
        (0, 18),
        "the next word, after the cursor"
    );
    editor.handle_key(key(KeyCode::Esc));
    editor.handle_key(alt(KeyCode::Char(';')));
    assert_eq!(editor.cursor_position(), (0, 6), "a wrap at the end");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn add_keeps_the_word_in_the_personal_dictionary_and_ignore_only_in_this_run() {
    let (path, mut editor) = spell_editor("spelladd", "zzone and zztwo\n");
    editor.handle_key(alt(KeyCode::Char(';')));
    for _ in 0..2 {
        editor.handle_key(key(KeyCode::Down)); // past the two suggestions: "Add"
    }
    editor.handle_key(key(KeyCode::Enter));
    assert!(editor.message.starts_with("Added"), "{}", editor.message);
    let words = path.parent().unwrap().join("words.txt");
    assert_eq!(fs::read_to_string(&words).unwrap(), "zzone\n");
    assert_eq!(
        editor.spelling.marks[0],
        [(10, 15)],
        "zzone is not marked any more"
    );
    editor.handle_key(alt(KeyCode::Char(';')));
    for _ in 0..3 {
        editor.handle_key(key(KeyCode::Down)); // "Ignore"
    }
    editor.handle_key(key(KeyCode::Enter));
    assert!(editor.spelling.marks[0].is_empty());
    assert_eq!(
        fs::read_to_string(&words).unwrap(),
        "zzone\n",
        "ignore does not save"
    );
    assert!(editor.spelling.known.contains("zztwo"));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn without_the_program_the_check_says_so_and_does_not_repeat() {
    let path = temp_file("spellmissing", "hello\n");
    let mut editor = open(&path);
    editor.spelling.program = "/nonexistent/hunspell".into();
    editor.handle_key(key(KeyCode::F(7)));
    assert!(editor.spell_on());
    wait_for_spell(&mut editor);
    assert!(
        editor.message.contains("not installed"),
        "{}",
        editor.message
    );
    assert!(editor.spelling.job.is_none());
    editor.handle_key(alt(KeyCode::Char(';')));
    assert!(
        editor.message.contains("not ready") || editor.message.contains("No misspelled"),
        "{}",
        editor.message
    );
    editor.handle_key(key(KeyCode::F(7)));
    assert!(!editor.spell_on());
    editor.handle_key(alt(KeyCode::Char(';')));
    assert!(editor.message.contains("F7"), "{}", editor.message);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

const THREE_PAGES: &str = "#set page(width: 8cm, height: 5cm)\n= One\ntext one\n#pagebreak()\n== Two A\ntext two\n#pagebreak()\n= Three\ntext three\n";

/// An editor on the three page document, with the preview that follows the cursor, after the first
/// compile and the answer about the headings.
fn follow_editor(name: &str) -> (PathBuf, Editor) {
    let path = temp_file(name, THREE_PAGES);
    let mut editor = open(&path);
    editor.set_follow(true);
    editor.compile_now();
    wait_for_idle(&mut editor);
    let start = Instant::now();
    while editor.follow.document.is_empty() {
        editor.tick(Instant::now());
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "no answer about the headings"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    (path, editor)
}

#[test]
fn the_preview_goes_to_the_page_of_the_section_when_the_cursor_enters_it() {
    let (path, mut editor) = follow_editor("followpage");
    let pages: Vec<usize> = editor
        .follow
        .document
        .iter()
        .map(|heading| heading.page)
        .collect();
    assert_eq!(
        pages,
        [1, 2, 4].map(|page| page.min(3)),
        "the page of `= Three` is 3"
    );
    assert_eq!(
        editor.preview.wanted_page(),
        1,
        "the cursor is in the first section"
    );
    editor.set_cursor_position((4, 0)); // "text two"
    editor.tick(Instant::now());
    assert_eq!(editor.preview.wanted_page(), 2);
    assert!(editor.compiling(), "the compile for the page started");
    wait_for_idle(&mut editor);
    editor.set_cursor_position((8, 0)); // "text three"
    editor.tick(Instant::now());
    assert_eq!(editor.preview.wanted_page(), 3);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_page_that_the_user_chose_stays_until_the_cursor_enters_another_section() {
    let (path, mut editor) = follow_editor("followstay");
    editor.set_cursor_position((4, 0));
    editor.tick(Instant::now());
    wait_for_idle(&mut editor);
    editor.handle_key(alt(KeyCode::Down)); // the user turns to page 3
    assert_eq!(editor.preview.wanted_page(), 3);
    wait_for_idle(&mut editor);
    editor.set_cursor_position((5, 2)); // the same section
    editor.tick(Instant::now());
    assert_eq!(
        editor.preview.wanted_page(),
        3,
        "no jump inside the section"
    );
    editor.set_cursor_position((1, 0)); // the first section
    editor.tick(Instant::now());
    assert_eq!(editor.preview.wanted_page(), 1);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn f3_switches_the_follow_off_and_on() {
    let (path, mut editor) = follow_editor("followkey");
    editor.handle_key(key(KeyCode::F(3)));
    assert!(!editor.follow_on());
    assert_eq!(editor.message, "The preview stays on its page");
    editor.set_cursor_position((8, 0));
    editor.tick(Instant::now());
    assert_eq!(editor.preview.wanted_page(), 1, "off: no jump");
    editor.handle_key(key(KeyCode::F(3)));
    assert!(editor.follow_on());
    editor.tick(Instant::now());
    assert_eq!(
        editor.preview.wanted_page(),
        3,
        "on again: the section of the cursor counts"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// The compile pane as text: the screen rows of the pane in the left half, from its title to its bottom edge.
fn pane_rows(editor: &mut Editor) -> Vec<String> {
    // The pane stands under the text area: the last 6 rows above the status line.
    let rows = screen_rows(editor);
    rows[rows.len() - 7..rows.len() - 1]
        .iter()
        .map(|row| row.chars().take(50).collect())
        .collect()
}

#[test]
fn a_good_compile_shows_its_time_in_the_first_line() {
    let path = temp_file("timeok", "text\n");
    let mut editor = open(&path);
    editor.report = Some(Report::new(true, vec![]).with_elapsed(Duration::from_millis(310)));
    let rows = pane_rows(&mut editor);
    assert!(rows[1].contains("OK in 310 ms"), "{rows:?}");

    editor.report = Some(Report::new(true, vec![]));
    assert!(
        pane_rows(&mut editor)[1].starts_with("│OK "),
        "OK without a time stays OK"
    );
    assert!(!pane_rows(&mut editor)[1].contains(" in "));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_failed_compile_shows_its_time_in_the_title() {
    let path = temp_file("timefail", "text\n");
    let mut editor = open(&path);
    editor.report = Some(
        Report::new(false, vec!["a.typ:1:1: error: x".into()])
            .with_elapsed(Duration::from_millis(1234)),
    );
    let rows = pane_rows(&mut editor);
    assert!(
        rows[0].contains("Compile") && rows[0].contains("(1234 ms)"),
        "{rows:?}"
    );

    editor.report = Some(Report::failed("Cannot run typst"));
    assert!(
        !pane_rows(&mut editor)[0].contains("ms"),
        "no time for a command that did not run"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_real_compile_shows_its_time() {
    let path = temp_file("timereal", "= Title\n");
    let mut editor = open(&path);
    compile_and_wait(&mut editor);
    let rows = pane_rows(&mut editor);
    assert!(
        rows[1].starts_with("│OK in ") && rows[1].contains(" ms"),
        "{rows:?}"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_counts_use_the_right_words() {
    assert_eq!(counts_text(0, 0), None);
    assert_eq!(counts_text(1, 0).as_deref(), Some("1 error"));
    assert_eq!(counts_text(2, 0).as_deref(), Some("2 errors"));
    assert_eq!(counts_text(0, 1).as_deref(), Some("1 warning"));
    assert_eq!(counts_text(0, 5).as_deref(), Some("5 warnings"));
    assert_eq!(counts_text(2, 1).as_deref(), Some("2 errors, 1 warning"));
    assert_eq!(counts_text(1, 3).as_deref(), Some("1 error, 3 warnings"));
}

fn report_with(ok: bool, lines: &[&str]) -> Option<Report> {
    Some(Report::new(
        ok,
        lines.iter().map(|line| line.to_string()).collect(),
    ))
}

#[test]
fn the_title_shows_the_number_of_errors_and_warnings() {
    let path = temp_file("counts", "text\n");
    let mut editor = open(&path);
    editor.report = report_with(
        false,
        &[
            "a.typ:1:1: error: e1",
            "a.typ:2:1: error: e2",
            "a.typ:3:1: warning: w",
        ],
    );
    assert!(
        pane_rows(&mut editor)[0].starts_with("┌Compile: 2 errors, 1 warning"),
        "{:?}",
        pane_rows(&mut editor)
    );

    editor.report = report_with(false, &["a.typ:1:1: error: e1"]);
    assert!(
        pane_rows(&mut editor)[0].starts_with("┌Compile: 1 error─"),
        "{:?}",
        pane_rows(&mut editor)
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_title_has_the_counts_and_the_time() {
    let path = temp_file("countstime", "text\n");
    let mut editor = open(&path);
    editor.report = Some(
        Report::new(false, vec!["a.typ:1:1: error: e".into()])
            .with_elapsed(Duration::from_millis(310)),
    );
    assert!(
        pane_rows(&mut editor)[0].starts_with("┌Compile: 1 error (310 ms)"),
        "{:?}",
        pane_rows(&mut editor)
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_warning_without_an_error_shows_the_count_and_the_pane_is_not_red() {
    let path = temp_file("warnonly", "text\n");
    let mut editor = open(&path);
    editor.report = report_with(true, &["a.typ:1:1: warning: unknown font family: x"]);
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| editor.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    let title: String = (0..50)
        .map(|column| buffer[(column, 17)].symbol())
        .collect();
    assert!(title.starts_with("┌Compile: 1 warning"), "{title:?}");
    let border = buffer[(0, 17)].fg;
    assert_ne!(
        border,
        Color::Red,
        "a compile with warnings only is not red"
    );
    assert_eq!(border, Color::Green);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn without_errors_and_warnings_the_title_stays_compile() {
    let path = temp_file("nocounts", "text\n");
    let mut editor = open(&path);
    editor.report = report_with(true, &[]);
    assert!(
        pane_rows(&mut editor)[0].starts_with("┌Compile─"),
        "{:?}",
        pane_rows(&mut editor)
    );
    editor.report = report_with(false, &["Cannot run typst: not found"]);
    assert!(
        pane_rows(&mut editor)[0].starts_with("┌Compile─"),
        "{:?}",
        pane_rows(&mut editor)
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_real_compile_with_a_warning_shows_the_warning_count() {
    let path = temp_file("realwarn", "#set text(font: \"NoSuchFontAtAll\")\nHello\n");
    let mut editor = open(&path);
    compile_and_wait(&mut editor);
    assert!(editor.report.as_ref().unwrap().ok);
    assert!(
        pane_rows(&mut editor)[0].starts_with("┌Compile: 1 warning"),
        "{:?}",
        pane_rows(&mut editor)
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// The first cell of the screen row that holds `text`, at the left of the compile pane.
fn pane_cell(editor: &mut Editor, text: &str) -> ratatui::buffer::Cell {
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| editor.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    for row in 17..24 {
        let line: String = (0..50)
            .map(|column| buffer[(column, row)].symbol())
            .collect();
        if let Some(at) = line.find(text) {
            let column = line[..at].chars().count();
            return buffer[(column as u16, row)].clone();
        }
    }
    panic!("the text {text:?} is not in the pane");
}

#[test]
fn an_error_a_warning_and_a_hint_have_different_looks() {
    let path = temp_file("colors", "text\n");
    let mut editor = open(&path);
    editor.report = report_with(
        false,
        &[
            "a.typ:1:1: error: boom",
            "a.typ:2:1: warning: careful",
            "hint: try a space",
        ],
    );
    let error = pane_cell(&mut editor, "a.typ:1:1");
    let warning = pane_cell(&mut editor, "a.typ:2:1");
    let hint = pane_cell(&mut editor, "hint:");
    assert_eq!(error.fg, Color::Red);
    assert_eq!(warning.fg, Color::Yellow);
    assert_ne!(error.fg, warning.fg);
    assert!(
        hint.modifier.contains(ratatui::style::Modifier::DIM),
        "a hint must be dim"
    );
    assert_eq!(hint.fg, Color::Reset, "a hint has no color of its own");
    assert!(!error.modifier.contains(ratatui::style::Modifier::DIM));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn without_colors_no_cell_outside_the_preview_has_a_color_and_the_state_stays_clear() {
    let path = temp_file("nocolor", "text\n");
    let mut editor = open(&path);
    editor.colors = false;
    editor.report = report_with(
        false,
        &["a.typ:1:1: error: boom", "a.typ:2:1: warning: careful"],
    );
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| editor.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    for row in 0..24 {
        for column in 0..50 {
            let cell = &buffer[(column, row)];
            assert_eq!(
                (cell.fg, cell.bg),
                (Color::Reset, Color::Reset),
                "cell {column},{row} {:?}",
                cell.symbol()
            );
        }
    }
    let text = screen_text(&mut editor);
    assert!(text.contains("Compile: 1 error, 1 warning"), "{text}");
    let error = pane_cell(&mut editor, "a.typ:1:1");
    assert!(error.modifier.contains(ratatui::style::Modifier::BOLD));
    editor.job = None;
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_variable_no_color_turns_colors_off_only_when_it_is_not_empty() {
    let value = |text: &str| Some(std::ffi::OsString::from(text));
    assert!(colors_wanted_for(None));
    assert!(colors_wanted_for(value("")));
    assert!(!colors_wanted_for(value("1")));
}

#[test]
fn the_colors_are_palette_colors_so_that_they_follow_the_theme() {
    let path = temp_file("palette", "text\n");
    let mut editor = open(&path);
    editor.report = report_with(
        false,
        &["a.typ:1:1: error: boom", "a.typ:2:1: warning: careful"],
    );
    for text in ["a.typ:1:1", "a.typ:2:1"] {
        let cell = pane_cell(&mut editor, text);
        assert!(
            !matches!(cell.fg, Color::Rgb(..) | Color::Indexed(..)),
            "{:?}",
            cell.fg
        );
    }
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_ok_line_and_the_export_line_have_no_color_of_their_own() {
    let path = temp_file("plainlines", "text\n");
    let mut editor = open(&path);
    editor.report = Some(Report::new(true, vec![]).with_elapsed(Duration::from_millis(5)));
    editor.exported = Some(path.with_extension("pdf"));
    assert_eq!(pane_cell(&mut editor, "OK in").fg, Color::Reset);
    assert_eq!(pane_cell(&mut editor, "Exported").fg, Color::Reset);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn wrap_rows_keeps_a_short_line_in_one_row() {
    assert_eq!(wrap_rows("short line", 20), ["short line"]);
    assert_eq!(wrap_rows("", 20), [""], "an empty line keeps one row");
    assert_eq!(wrap_rows("exactly ten", 11), ["exactly ten"]);
}

#[test]
fn wrap_rows_breaks_at_a_space_when_it_can() {
    assert_eq!(wrap_rows("aaa bbb ccc", 7), ["aaa bbb", "ccc"]);
    assert_eq!(wrap_rows("aaa bbb ccc", 8), ["aaa bbb", "ccc"]);
    assert_eq!(wrap_rows("aaa bbb ccc", 3), ["aaa", "bbb", "ccc"]);
}

#[test]
fn wrap_rows_splits_a_word_that_is_wider_than_the_row() {
    assert_eq!(wrap_rows("abcdefghij", 4), ["abcd", "efgh", "ij"]);
    assert_eq!(wrap_rows("ab abcdefghij", 4), ["ab", "abcd", "efgh", "ij"]);
}

#[test]
fn wrap_rows_counts_the_display_width_of_wide_characters() {
    assert_eq!(
        wrap_rows("中中中", 5),
        ["中中", "中"],
        "each of these characters is 2 cells wide"
    );
    assert_eq!(wrap_rows("é é é", 3), ["é é", "é"]);
}

#[test]
fn wrap_rows_with_no_width_gives_no_rows() {
    assert!(wrap_rows("text", 0).is_empty());
}

/// A report of `count` lines `hint: line <n>`. Each line is short and takes one row.
fn hints(count: usize) -> Option<Report> {
    Some(Report::new(
        false,
        (1..=count).map(|n| format!("hint: line {n}")).collect(),
    ))
}

#[test]
fn a_report_of_seven_short_lines_shows_three_lines_and_four_more() {
    let path = temp_file("more7", "text\n");
    let mut editor = open(&path);
    editor.report = hints(7);
    let rows = pane_rows(&mut editor);
    assert!(
        rows[1].contains("hint: line 1")
            && rows[2].contains("hint: line 2")
            && rows[3].contains("hint: line 3"),
        "{rows:?}"
    );
    assert!(rows[4].contains("+4 more"), "{rows:?}");
    assert!(
        !rows.iter().any(|row| row.contains("line 4")),
        "a hidden line shows: {rows:?}"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_report_that_fits_shows_no_count() {
    let path = temp_file("morefit", "text\n");
    let mut editor = open(&path);
    for count in [1, 3, 4] {
        editor.report = hints(count);
        let rows = pane_rows(&mut editor);
        assert!(
            !rows.iter().any(|row| row.contains("more")),
            "{count} lines: {rows:?}"
        );
        assert!(
            rows[count].contains(&format!("hint: line {count}")),
            "{rows:?}"
        );
    }
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn five_rows_show_three_and_two_more() {
    let path = temp_file("more5", "text\n");
    let mut editor = open(&path);
    editor.report = hints(5);
    let rows = pane_rows(&mut editor);
    assert!(
        rows[3].contains("hint: line 3") && rows[4].contains("+2 more"),
        "{rows:?}"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_count_is_in_screen_rows_after_the_wrap_and_not_in_report_lines() {
    let path = temp_file("morewrap", "text\n");
    let mut editor = open(&path);
    // One report line of 6 words. Each word has 40 characters, so at 48 columns each word takes one row.
    let word = "w".repeat(40);
    let long = [word.as_str(); 6].join(" ");
    editor.report = Some(Report::new(false, vec![long]));
    let rows = pane_rows(&mut editor);
    assert!(rows[4].contains("+3 more"), "6 rows, 3 shown: {rows:?}");
    assert!(
        rows[1].contains(&word) && rows[3].contains(&word),
        "{rows:?}"
    );
    // The same text on 2 report lines is also 6 rows, and the count is the same.
    editor.report = Some(Report::new(
        false,
        vec![[word.as_str(); 3].join(" "), [word.as_str(); 3].join(" ")],
    ));
    assert!(
        pane_rows(&mut editor)[4].contains("+3 more"),
        "{:?}",
        pane_rows(&mut editor)
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_ok_line_and_the_export_line_count_as_rows() {
    let path = temp_file("morehead", "text\n");
    let mut editor = open(&path);
    editor.report = Some(
        Report::new(
            true,
            (1..=4)
                .map(|n| format!("a.typ:{n}:1: warning: w{n}"))
                .collect(),
        )
        .with_elapsed(Duration::from_millis(5)),
    );
    editor.exported = Some(path.with_extension("pdf"));
    // OK line + 4 warnings + export line = 6 rows. 3 are shown.
    let rows = pane_rows(&mut editor);
    assert!(
        rows[1].contains("OK in 5 ms") && rows[4].contains("+3 more"),
        "{rows:?}"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_count_is_dim() {
    let path = temp_file("moredim", "text\n");
    let mut editor = open(&path);
    editor.report = hints(9);
    let cell = pane_cell(&mut editor, "+6 more");
    assert!(cell.modifier.contains(ratatui::style::Modifier::DIM));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_very_narrow_screen_does_not_panic_and_keeps_the_count() {
    let path = temp_file("morenarrow", "text\n");
    let mut editor = open(&path);
    editor.report = hints(9);
    for width in [1, 2, 3, 6, 12] {
        let mut terminal = Terminal::new(TestBackend::new(width, 8)).unwrap();
        terminal.draw(|frame| editor.draw(frame)).unwrap();
    }
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_pane_says_compiling_before_the_first_report() {
    let path = temp_file("pane-first", "= Title\n");
    let mut editor = open(&path);
    assert!(screen_text(&mut editor).contains("Press Ctrl-B to compile."));

    editor.handle_key(ctrl('b'));
    let text = screen_text(&mut editor);
    assert!(
        text.contains("Compile (running)") && text.contains("Compiling..."),
        "{text}"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_pane_keeps_the_last_report_while_a_compile_runs() {
    let path = temp_file("pane-keep", "= Title\n");
    let mut editor = open(&path);
    editor.report = Some(Report::failed("old error text"));

    editor.handle_key(ctrl('b'));
    let text = screen_text(&mut editor);
    assert!(text.contains("Compile (running)"), "{text}");
    assert!(text.contains("old error text"), "{text}");
    assert!(!text.contains("Compiling..."), "{text}");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn only_the_folder_of_the_last_compile_stays() {
    let path = temp_file("folders", "= Title\n");
    let mut editor = open(&path);
    let pages = path.parent().unwrap().join("pages");

    editor.handle_key(ctrl('b'));
    editor.handle_key(ctrl('b')); // kills the first compile and deletes its folder
    wait_for_report(&mut editor);
    editor.handle_key(ctrl('b'));
    wait_for_report(&mut editor);
    assert!(editor.report.as_ref().unwrap().ok);
    assert_eq!(fs::read_dir(&pages).unwrap().count(), 1);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_failed_compile_leaves_no_folder() {
    let path = temp_file("nofolder", "#nope()\n");
    let mut editor = open(&path);
    let pages = path.parent().unwrap().join("pages");
    editor.handle_key(ctrl('b'));
    wait_for_report(&mut editor);
    assert!(!editor.report.as_ref().unwrap().ok);
    assert_eq!(fs::read_dir(&pages).unwrap().count(), 0);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

fn alt(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::ALT)
}

#[test]
fn alt_down_and_alt_up_render_the_new_page_with_a_new_compile() {
    let path = temp_file(
        "pages",
        "= One\n#pagebreak()\n= Two\n#pagebreak()\n= Three\n",
    );
    let mut editor = open(&path);
    editor.handle_key(ctrl('b'));
    wait_for_report(&mut editor);
    assert!(screen_text(&mut editor).contains("Preview 1/3"));

    editor.handle_key(alt(KeyCode::Down));
    assert!(editor.job.is_some(), "a page turn must start a compile");
    assert!(
        screen_text(&mut editor).contains("Preview 1/3"),
        "the old page must stay until the new page is ready"
    );
    wait_for_report(&mut editor);
    assert!(screen_text(&mut editor).contains("Preview 2/3"));

    // Fast presses replace each other: the last press decides the page.
    editor.handle_key(alt(KeyCode::Down));
    editor.handle_key(alt(KeyCode::Down));
    wait_for_report(&mut editor);
    assert!(screen_text(&mut editor).contains("Preview 3/3"));

    editor.handle_key(alt(KeyCode::Down));
    assert!(
        editor.job.is_none(),
        "page 3 is the last page, so no compile starts"
    );
    editor.handle_key(alt(KeyCode::Up));
    wait_for_report(&mut editor);
    assert!(screen_text(&mut editor).contains("Preview 2/3"));
    assert!(!editor.dirty, "the page keys must not change the text");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// Waits up to 30 seconds until no compile runs. A recovery compile starts the next compile at once.
fn wait_for_idle(editor: &mut Editor) {
    let start = Instant::now();
    while editor.job.is_some() {
        editor.tick(Instant::now());
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "the compiles did not end"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Shows page 3 of a document of 3 pages. Then another program makes the document `new_text`, and Ctrl-B compiles it.
fn shrink_from_page_3(name: &str, new_text: &str) -> (PathBuf, Editor) {
    let path = temp_file(name, "= One\n#pagebreak()\n= Two\n#pagebreak()\n= Three\n");
    let mut editor = open(&path);
    editor.handle_key(ctrl('b'));
    wait_for_idle(&mut editor);
    editor.handle_key(alt(KeyCode::Down));
    editor.handle_key(alt(KeyCode::Down));
    wait_for_idle(&mut editor);
    assert!(screen_text(&mut editor).contains("Preview 3/3"));

    fs::write(&path, new_text).unwrap();
    editor.handle_key(ctrl('b')); // the buffer is clean, so this compiles the new text from the disk
    wait_for_idle(&mut editor);
    (path, editor)
}

#[test]
fn a_document_that_gets_shorter_than_the_wanted_page_shows_the_last_page() {
    let (path, mut editor) = shrink_from_page_3("shrink2", "= One\n#pagebreak()\n= Two\n");
    let text = screen_text(&mut editor);
    assert!(text.contains("Preview 2/2"), "{text}");
    assert!(editor.report.as_ref().unwrap().ok);
    assert_eq!(editor.preview.wanted_page(), 2);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_document_that_gets_down_to_one_page_shows_that_page() {
    let (path, mut editor) = shrink_from_page_3("shrink1", "= Only\n");
    let text = screen_text(&mut editor);
    assert!(text.contains("Preview 1/1"), "{text}");
    assert!(editor.report.as_ref().unwrap().ok);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_compile_that_gives_success_and_no_page_for_page_1_is_an_error_and_never_loops() {
    let path = temp_file("nopage", "= One\n");
    let mut editor = open(&path);
    let dir = path.parent().unwrap().join("empty-dir");
    fs::create_dir_all(&dir).unwrap();
    editor.job = Some(Job::ended_with_success(dir));
    wait_for_report(&mut editor);
    assert!(editor.job.is_none(), "a new compile started");
    let report = editor.report.as_ref().unwrap();
    assert!(
        !report.ok && report.lines[0].contains("no page file"),
        "{:?}",
        report.lines
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_recovery_that_finds_no_page_stops_with_an_error() {
    let (path, mut editor) = shrink_from_page_3("noloop", "= One\n#pagebreak()\n= Two\n");
    editor.preview.want(9);
    editor.recover = Some(9); // a recovery is under way, and now the compile of page 1 gives no file
    let dir = path.parent().unwrap().join("empty-dir2");
    fs::create_dir_all(&dir).unwrap();
    editor.job = Some(Job::ended_with_success(dir));
    wait_for_report(&mut editor);
    assert!(editor.job.is_none(), "the recovery looped");
    assert!(editor.recover.is_none());
    assert!(!editor.report.as_ref().unwrap().ok);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_page_turn_ends_a_recovery() {
    let path = temp_file(
        "turnrecover",
        "= One\n#pagebreak()\n= Two\n#pagebreak()\n= Three\n",
    );
    let mut editor = open(&path);
    editor.handle_key(ctrl('b'));
    wait_for_idle(&mut editor);
    editor.recover = Some(9);
    editor.handle_key(alt(KeyCode::Down));
    assert!(
        editor.recover.is_none(),
        "the recovery must end when the user turns a page"
    );
    wait_for_idle(&mut editor);
    assert!(
        screen_text(&mut editor).contains("Preview 2/3"),
        "the user's page turn must win"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn alt_end_and_alt_home_show_the_last_page_and_the_first_page() {
    let path = temp_file(
        "jump",
        "= One\n#pagebreak()\n= Two\n#pagebreak()\n= Three\n",
    );
    let mut editor = open(&path);
    editor.handle_key(ctrl('b'));
    wait_for_idle(&mut editor);
    assert!(screen_text(&mut editor).contains("Preview 1/3"));

    editor.handle_key(alt(KeyCode::End));
    assert!(editor.job.is_some(), "the jump must start a compile");
    assert!(
        screen_text(&mut editor).contains("Preview 1/3"),
        "the old page stays until the new page is ready"
    );
    wait_for_idle(&mut editor);
    assert!(screen_text(&mut editor).contains("Preview 3/3"));

    editor.handle_key(alt(KeyCode::End));
    assert!(editor.job.is_none(), "already on the last page");
    editor.handle_key(alt(KeyCode::Home));
    wait_for_idle(&mut editor);
    assert!(screen_text(&mut editor).contains("Preview 1/3"));
    assert!(!editor.dirty, "the jump keys must not change the text");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_page_jump_keys_do_nothing_before_the_first_compile() {
    let path = temp_file("jumpearly", "= One\n");
    let mut editor = open(&path);
    editor.handle_key(alt(KeyCode::End));
    editor.handle_key(alt(KeyCode::Home));
    assert!(editor.job.is_none());
    assert!(!editor.dirty);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn plain_home_and_end_still_move_the_cursor_in_the_text() {
    let path = temp_file("homeend", "some text\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::End));
    assert_eq!(editor.cursor_position(), (0, 9));
    editor.handle_key(key(KeyCode::Home));
    assert_eq!(editor.cursor_position(), (0, 0));
    assert!(editor.job.is_none());
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_page_jump_ends_a_recovery() {
    let path = temp_file("jumprecover", "= One\n#pagebreak()\n= Two\n");
    let mut editor = open(&path);
    editor.handle_key(ctrl('b'));
    wait_for_idle(&mut editor);
    editor.recover = Some(9);
    editor.handle_key(alt(KeyCode::End));
    assert!(editor.recover.is_none());
    wait_for_idle(&mut editor);
    assert!(screen_text(&mut editor).contains("Preview 2/2"));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_page_turn_works_while_the_file_has_a_conflict_and_does_not_save() {
    let path = temp_file("turnconflict", "= One\n#pagebreak()\n= Two\n");
    let mut editor = open(&path);
    editor.handle_key(ctrl('b'));
    wait_for_report(&mut editor);

    editor.handle_key(key(KeyCode::Char('X')));
    fs::write(&path, "= One\n#pagebreak()\n= Two\nchanged outside\n").unwrap();
    editor.handle_key(alt(KeyCode::Down));
    assert!(editor.job.is_some(), "the turn must start a compile");
    assert!(editor.dirty, "the turn must not save");
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "= One\n#pagebreak()\n= Two\nchanged outside\n"
    );
    wait_for_report(&mut editor);
    assert!(screen_text(&mut editor).contains("Preview 2/2"));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_page_number_stays_after_an_edit() {
    let path = temp_file(
        "pageedit",
        "= One\n#pagebreak()\n= Two\n#pagebreak()\n= Three\n",
    );
    let mut editor = open(&path);
    editor.handle_key(ctrl('b'));
    wait_for_report(&mut editor);
    editor.handle_key(alt(KeyCode::Down));
    wait_for_report(&mut editor);
    assert!(screen_text(&mut editor).contains("Preview 2/3"));

    editor.handle_key(key(KeyCode::Char('X')));
    assert!(editor.tick(Instant::now() + Duration::from_millis(400)));
    wait_for_report(&mut editor);
    assert!(
        screen_text(&mut editor).contains("Preview 2/3"),
        "the live compile must render page 2"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn each_compile_folder_holds_exactly_one_png() {
    let path = temp_file(
        "onepng",
        "= One\n#pagebreak()\n= Two\n#pagebreak()\n= Three\n",
    );
    let mut editor = open(&path);
    editor.handle_key(ctrl('b'));
    wait_for_report(&mut editor);
    editor.handle_key(alt(KeyCode::Down));
    wait_for_report(&mut editor);

    let folders: Vec<_> = fs::read_dir(&editor.pages_root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(folders.len(), 1, "only the folder of the shown page stays");
    let files: Vec<_> = fs::read_dir(&folders[0])
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .filter(|name| name != "deps")
        .collect();
    assert_eq!(files, ["page-2-of-3.png"]);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn an_error_on_a_later_page_shows_while_page_1_is_wanted() {
    let path = temp_file(
        "laterror",
        "= One\n#pagebreak()\n= Two\n#pagebreak()\n#nope()\n",
    );
    let mut editor = open(&path);
    editor.handle_key(ctrl('b'));
    wait_for_report(&mut editor);
    let report = editor.report.as_ref().unwrap();
    assert!(!report.ok);
    assert!(
        report.lines.iter().any(|l| l.contains(":5:")),
        "{:?}",
        report.lines
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// Waits up to 20 seconds for the running PDF export to finish.
fn wait_for_export(editor: &mut Editor) {
    let start = Instant::now();
    while editor.export.is_some() {
        editor.tick(Instant::now());
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "export did not finish"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn ctrl_e_saves_then_exports_a_pdf_next_to_the_file() {
    let path = temp_file("export", "= Title\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    editor.handle_key(ctrl('e'));
    assert!(!editor.dirty, "Ctrl-E must save first");
    assert_eq!(fs::read_to_string(&path).unwrap(), "X= Title\n");
    assert!(editor.export.is_some());

    wait_for_export(&mut editor);
    let pdf = path.with_extension("pdf");
    assert_eq!(&fs::read(&pdf).unwrap()[..4], b"%PDF");
    assert_eq!(editor.exported.as_deref(), Some(pdf.as_path()));
    assert!(screen_text(&mut editor).contains("doc.pdf"));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_export_path_stays_on_screen_after_a_later_compile() {
    let path = temp_file("exportstays", "= Title\n");
    let mut editor = open(&path);
    editor.handle_key(ctrl('e'));
    wait_for_export(&mut editor);
    editor.handle_key(ctrl('b'));
    wait_for_report(&mut editor);
    let text = screen_text(&mut editor);
    assert!(text.contains("OK"), "{text}");
    assert!(
        text.contains("Exported") && text.contains("doc.pdf"),
        "{text}"
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_status_line_reports_the_end_of_the_export() {
    let path = temp_file("exportstatus", "= Title\n");
    let mut editor = open(&path);
    editor.handle_key(ctrl('e'));
    assert!(editor.message.contains("Exporting"));
    wait_for_export(&mut editor);
    assert!(!editor.message.contains("Exporting"), "{}", editor.message);
    assert!(editor.message.contains("doc.pdf"), "{}", editor.message);

    fs::write(&path, "#nope()\n").unwrap();
    let mut editor = open(&path);
    editor.handle_key(ctrl('e'));
    wait_for_export(&mut editor);
    assert!(editor.message.contains("failed"), "{}", editor.message);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_failed_export_shows_the_errors_and_writes_no_file() {
    let path = temp_file("exporterr", "#nope()\n");
    let mut editor = open(&path);
    editor.handle_key(ctrl('e'));
    wait_for_export(&mut editor);
    let report = editor.report.as_ref().unwrap();
    assert!(!report.ok);
    assert!(
        report.lines.iter().any(|l| l.contains(":1:")),
        "{:?}",
        report.lines
    );
    assert!(!path.with_extension("pdf").exists());
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn plain_n_and_p_type_letters() {
    let path = temp_file("letters", "");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('n')));
    editor.handle_key(key(KeyCode::Char('p')));
    assert_eq!(editor.textarea.lines(), ["np"]);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn ctrl_b_saves_then_compiles() {
    let path = temp_file("build", "= Title\n");
    let mut editor = open(&path);
    editor.handle_key(key(KeyCode::Char('X')));
    editor.handle_key(ctrl('b'));
    assert!(!editor.dirty, "Ctrl-B must save first");
    assert_eq!(fs::read_to_string(&path).unwrap(), "X= Title\n");
    assert!(editor.job.is_some());

    wait_for_report(&mut editor);
    assert!(editor.job.is_none());
    let report = editor.report.as_ref().unwrap();
    assert!(report.ok, "{:?}", report.lines);
    assert!(editor.preview.has_page(), "the page did not load");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_compile_error_reaches_the_report() {
    let path = temp_file("builderr", "#nope()\n");
    let mut editor = open(&path);
    editor.handle_key(ctrl('b'));
    wait_for_report(&mut editor);
    let report = editor.report.as_ref().unwrap();
    assert!(!report.ok);
    assert!(
        report.lines.iter().any(|l| l.contains(":1:")),
        "{:?}",
        report.lines
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
