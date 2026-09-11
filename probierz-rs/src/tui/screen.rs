//! What a human sees on the terminal: the text with its escape sequences removed,
//! and the last repaint frame rather than everything ever written.

/// Everything a terminal wrote, with the escape sequences that drew it
/// removed. A carriage return is dropped too: it moves the cursor, it is not
/// content.
pub fn strip_ansi(text: &str) -> String {
    let bytes: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0usize;
    while index < bytes.len() {
        let character = bytes[index];
        if character != '\u{1b}' {
            if character != '\r' {
                out.push(character);
            }
            index += 1;
            continue;
        }
        index += 1;
        if index >= bytes.len() {
            break;
        }
        match bytes[index] {
            // An operating-system command runs to BEL or ST.
            ']' => {
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == '\u{7}' {
                        index += 1;
                        break;
                    }
                    if bytes[index] == '\u{1b}' && bytes.get(index + 1) == Some(&'\\') {
                        index += 2;
                        break;
                    }
                    index += 1;
                }
            }
            // A control sequence runs to its final byte in @-~.
            '[' => {
                index += 1;
                while index < bytes.len() && !matches!(bytes[index], '@'..='~') {
                    index += 1;
                }
                index += 1;
            }
            // Character-set selection takes one more byte; so do the two
            // keypad-mode sequences.
            '(' | ')' => index += 2,
            '=' | '>' => index += 1,
            _ => index += 1,
        }
    }
    out
}

/// The last repaint frame: what is on the screen now, rather than everything
/// the application has ever written.
pub(super) fn last_frame(raw: &str) -> String {
    let mut start = 0usize;
    let bytes = raw.as_bytes();
    let mut index = 0usize;
    while index + 2 < bytes.len() {
        if bytes[index] == 0x1b && bytes[index + 1] == b'[' {
            let mut cursor = index + 2;
            while cursor < bytes.len() && matches!(bytes[cursor], b'0'..=b'9' | b';' | b'?') {
                cursor += 1;
            }
            if cursor < bytes.len() && bytes[cursor] == b'J' {
                start = cursor + 1;
                index = cursor + 1;
                continue;
            }
        }
        index += 1;
    }
    strip_ansi(&raw[start.min(raw.len())..])
}
