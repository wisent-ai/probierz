use crate::specs::*;

pub(crate) fn at_iso(base: SystemTime, elapsed: Duration) -> String {
    iso_timestamp(base + elapsed)
}

/// An operator reads these lines on a terminal: keep the headline that says
/// what was expected and the tail that says what the application was showing.
/// A long screen dump never drowns the reason.
pub(crate) fn clip_row_error(text: &str) -> String {
    const LIMIT: usize = 2000;
    const HEAD: usize = 600;
    const TAIL: usize = 1400;
    if text.chars().count() <= LIMIT {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let head: String = chars[..HEAD].iter().collect();
    let tail: String = chars[chars.len() - TAIL..].iter().collect();
    format!("{head}\n...\n{tail}")
}
