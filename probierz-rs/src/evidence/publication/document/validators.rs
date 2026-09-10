use crate::evidence::*;

pub(crate) fn required_raw<'a>(
    value: Option<&'a str>,
    name: &str,
    predicate: impl Fn(&str) -> bool,
) -> Result<&'a str, Failure> {
    let value = value.unwrap_or_default();
    if value.is_empty() || !predicate(value) {
        Err(Failure::invalid(
            "evidence.onboarding_publication",
            format!("{name} is invalid"),
        ))
    } else {
        Ok(value)
    }
}

pub(crate) fn identifier(value: &str) -> bool {
    value.len() <= 128
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}
pub(crate) fn uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23].iter().all(|index| bytes[*index] == b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit())
        && matches!(bytes[14], b'1'..=b'5')
        && matches!(bytes[19].to_ascii_lowercase(), b'8' | b'9' | b'a' | b'b')
}
