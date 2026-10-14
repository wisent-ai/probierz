use crate::authoring::*;
/// Print `result` and answer whether it held, read at `verdict`, the JSON
/// pointer its producer writes its boolean verdict to: a result that did not
/// pass is a refusal of `point` carrying the whole result, never a process
/// exit, so the MCP server running the command in process keeps serving. A
/// result without that verdict is refused as such, never taken as passing.
pub fn print_result(point: &str, verdict: &str, result: JsonValue) -> crate::failure::Answer {
    let Some(ok) = result.pointer(verdict).and_then(JsonValue::as_bool) else {
        return Err(Failure::new(
            point,
            crate::failure::Code::Refused,
            format!("the result states no boolean verdict at {verdict}: {result}"),
        ));
    };
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
