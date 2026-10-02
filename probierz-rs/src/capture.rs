//! Where an answer goes instead of stdout while a caller in this process
//! collects it: the MCP server runs a command in-process and reads what it
//! answered, the way a shell would read its stdout. Only this thread's
//! answers are collected, so a run supervised on another thread keeps
//! printing where it printed.

use std::cell::RefCell;

thread_local! {
    static CAPTURE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Run `command` and answer what it printed instead of letting it reach
/// stdout.
pub fn captured<T>(command: impl FnOnce() -> T) -> (T, String) {
    CAPTURE.with(|sink| *sink.borrow_mut() = Some(String::new()));
    let result = command();
    let output = CAPTURE.with(|sink| sink.borrow_mut().take().unwrap_or_default());
    (result, output)
}

/// One answer: into the open sink, or to stdout when none is open.
pub fn emit(text: &str) {
    let kept = CAPTURE.with(|sink| {
        if let Some(buffer) = sink.borrow_mut().as_mut() {
            buffer.push_str(text);
            true
        } else {
            false
        }
    });
    if !kept {
        print!("{text}");
    }
}
