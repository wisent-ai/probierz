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
/// stdout. A capture inside another collects its own command's answers and
/// then gives the sink back, so the outer capture keeps what it had before;
/// opening a new sink over it lost the outer command's earlier answers.
pub fn captured<T>(command: impl FnOnce() -> T) -> (T, String) {
    let outer = CAPTURE.with(|sink| sink.replace(Some(String::new())));
    let result = command();
    let output = CAPTURE.with(|sink| sink.replace(outer));
    match output {
        Some(output) => (result, output),
        // Every capture restores the sink it found, so the one opened above
        // is the one this thread holds when `command` returns.
        None => unreachable!("a capture closed the sink it did not open"),
    }
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
