use crate::authoring::*;
/// Print `result` and answer whether it held: a result that did not pass is
/// a refusal of `point` carrying the whole result, never a process exit, so
/// the MCP server running the command in process keeps serving.
pub fn print_result(point: &str, result: JsonValue) -> crate::failure::Answer {
    let ok = result
        .get("ok")
        .and_then(JsonValue::as_bool)
        .or_else(|| result.pointer("/verdict/pass").and_then(JsonValue::as_bool))
        .or_else(|| result.get("pass").and_then(JsonValue::as_bool))
        .unwrap_or(true);
    print_json(&result)?;
    if ok {
        return Ok(());
    }
    Err(Failure::new(
        point,
        crate::failure::Code::Refused,
        result.to_string(),
    ))
}
