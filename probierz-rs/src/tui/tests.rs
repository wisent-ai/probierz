//! The terminal driver's unit tests.

use super::*;

#[test]
fn escape_sequences_are_removed_and_content_is_kept() {
    let painted = "\u{1b}[2J\u{1b}[H\u{1b}[1mReady\u{1b}[0m\r\nnext\u{1b}]0;title\u{7}";
    assert_eq!(strip_ansi(painted), "Ready\nnext");
}

#[test]
fn the_screen_is_the_last_repaint_not_the_whole_session() {
    let session = "first frame\u{1b}[2Jsecond frame";
    assert_eq!(last_frame(session), "second frame");
    assert_eq!(strip_ansi(session), "first framesecond frame");
}

#[test]
fn a_real_terminal_application_is_driven_and_read() {
    let terminal = Terminal::spawn(
        Spawn::new("/bin/sh")
            .arg("-c")
            .arg("printf 'hello from a terminal\\n'; sleep 5"),
    )
    .expect("spawn a terminal");
    let seen = terminal
        .wait_for("hello from a terminal", Duration::from_secs(10), true)
        .expect("the application's output");
    assert!(
        seen.contains("hello from a terminal"),
        "unexpected session: {seen}"
    );
    let (_, log) = terminal.close().expect("close the terminal");
    assert!(log.contains("hello from a terminal"));
}

#[test]
fn a_timeout_reports_the_screen_it_was_waiting_on() {
    let terminal = Terminal::spawn(
        Spawn::new("/bin/sh")
            .arg("-c")
            .arg("printf 'only this\\n'; sleep 5"),
    )
    .expect("spawn a terminal");
    let failure = terminal
        .wait_for("never appears", Duration::from_millis(400), false)
        .expect_err("the wait must time out");
    assert!(
        failure.detail.contains("never appears"),
        "detail: {}",
        failure.detail
    );
    let _ = terminal.close();
}

#[test]
fn an_unknown_key_is_refused_by_name() {
    let mut terminal =
        Terminal::spawn(Spawn::new("/bin/sh").arg("-c").arg("sleep 2")).expect("spawn a terminal");
    let failure = terminal
        .key("f13")
        .expect_err("unknown key must be refused");
    assert!(
        failure.detail.starts_with("unknown key: f13"),
        "detail: {}",
        failure.detail
    );
    let _ = terminal.close();
}
