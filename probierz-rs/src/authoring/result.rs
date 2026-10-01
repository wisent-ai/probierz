use crate::authoring::*;
pub fn print_result(result: JsonValue) -> Result<bool, Failure> {
    let ok = result
        .get("ok")
        .and_then(JsonValue::as_bool)
        .or_else(|| result.pointer("/verdict/pass").and_then(JsonValue::as_bool))
        .or_else(|| result.get("pass").and_then(JsonValue::as_bool))
        .unwrap_or(true);
    print_json(&result)?;
    Ok(ok)
}

