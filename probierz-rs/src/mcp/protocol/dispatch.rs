use serde_json::json;
use crate::*;
pub(crate) fn tool_answer(value: Value) -> Result<Value, String> {
    let pretty = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    Ok(json!({ "content": [{ "type": "text", "text": pretty }] }))
}

pub(crate) fn call_tool(
    control: &Arc<Control>,
    name: &str,
    args: &Map<String, Value>,
) -> Result<Value, String> {
    // The control tools are stateful for the lifetime of this MCP server.
    // Their worker still enters through `probierz run`, the product's one
    // execution path; only supervision and artifact reads live here.
    let controlled = match name {
        "probierz_start_run" => Some(control.start(args)),
        "probierz_run_status" => Some(control.status(args)),
        "probierz_cancel_run" => Some(control.cancel(args)),
        "probierz_get_result" => Some(control.result(args)),
        "probierz_list_artifacts" => Some(control.list_artifacts(args)),
        "probierz_get_artifact" => Some(control.get_artifact(args)),
        _ => None,
    };
    if let Some(result) = controlled {
        return tool_answer(result?);
    }

    // Read-only and side-effecting operations share transport, not authority:
    // no operation runs until this explicit call is routed. Discovery commands
    // remain the CLI's static, non-executing surfaces.
    let arguments = route(name, args)?;
    let output = Command::new(probierz_binary())
        .arg("--harness")
        .arg(harness_root())
        .args(&arguments)
        .output()
        .map_err(|error| format!("cannot run probierz: {error}"))?;
    if !output.stdout.is_empty() {
        let value: Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("probierz returned invalid JSON: {error}"))?;
        return tool_answer(value);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let message = stderr
        .lines()
        .last()
        .filter(|line| !line.is_empty())
        .unwrap_or("probierz command failed");
    Err(message.to_string())
}

pub(crate) fn handle(request: Value, tools: &Value, control: &Arc<Control>) {
    let Some(method) = request.get("method").and_then(Value::as_str) else {
        return;
    };
    if request.get("id").is_none() {
        return;
    }
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "probierz", "version": env!("CARGO_PKG_VERSION") },
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools })),
        "tools/call" => {
            let params = request.get("params").and_then(Value::as_object);
            let name = params.and_then(|value| non_empty(value.get("name"), "name").ok());
            match name {
                Some(name) => {
                    let empty = Map::new();
                    let args = params
                        .and_then(|value| value.get("arguments"))
                        .and_then(Value::as_object)
                        .unwrap_or(&empty);
                    call_tool(control, name, args)
                }
                None => Err("name must be a non-empty string".to_string()),
            }
        }
        _ => {
            send(
                &json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("method not found: {method}") } }),
            );
            return;
        }
    };
    match result {
        Ok(result) => send(&json!({ "jsonrpc": "2.0", "id": id, "result": result })),
        Err(message) => {
            let code = if message.starts_with("unknown tool:") {
                -32601
            } else {
                -32000
            };
            send(
                &json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
            )
        }
    }
}

/// Serve the MCP protocol on stdio until the client closes it.
pub(crate) fn serve() {
    let tools: Value = match serde_json::from_str(TOOLS_JSON) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("probierz-mcp tool contract is invalid: {error}");
            std::process::exit(1);
        }
    };
    let control = Arc::new(Control::default());
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else {
            break;
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(request) => handle(request, &tools, &control),
            Err(_) => send(
                &json!({ "jsonrpc": "2.0", "id": Value::Null, "error": { "code": -32700, "message": "parse error" } }),
            ),
        }
    }
    control.shutdown();
}
