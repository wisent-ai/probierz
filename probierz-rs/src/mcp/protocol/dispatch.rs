use crate::mcp::*;
use serde_json::json;
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
    // no operation runs until this explicit call is routed. The routed argv is
    // the CLI's own, run in this process; what the command answered is the
    // tool's answer, and its refusal is the tool's error.
    let arguments = route(name, args)?;
    let output = crate::run_in_process(&harness_root(), &arguments)?;
    let value: Value = serde_json::from_str(output.trim())
        .map_err(|error| format!("probierz answered invalid JSON: {error}"))?;
    tool_answer(value)
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
                    match params.and_then(|value| value.get("arguments")) {
                        Some(Value::Object(args)) => validate_tool_arguments(tools, name, args)
                            .and_then(|()| call_tool(control, name, args)),
                        None => validate_tool_arguments(tools, name, &empty)
                            .and_then(|()| call_tool(control, name, &empty)),
                        Some(_) => Err("arguments must be a JSON object".to_string()),
                    }
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

fn validate_tool_arguments(
    tools: &Value,
    name: &str,
    args: &Map<String, Value>,
) -> Result<(), String> {
    let declaration = tools
        .as_array()
        .ok_or_else(|| "MCP tool catalogue must be an array".to_string())?
        .iter()
        .find(|tool| tool.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| format!("unknown tool: {name}"))?;
    if let Some(required) = declaration.pointer("/inputSchema/required") {
        for field in required
            .as_array()
            .ok_or_else(|| format!("MCP tool {name} required fields must be an array"))?
        {
            let field = field
                .as_str()
                .ok_or_else(|| format!("MCP tool {name} required field must be text"))?;
            if !args.contains_key(field) {
                return Err(format!("{name} requires argument {field}"));
            }
        }
    }
    let properties = declaration
        .pointer("/inputSchema/properties")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("MCP tool {name} properties must be an object"))?;
    for (field, value) in args {
        if let Some(schema) = properties.get(field) {
            validate_argument_type(name, field, schema, value)?;
        }
    }
    Ok(())
}

fn validate_argument_type(
    tool: &str,
    field: &str,
    schema: &Value,
    value: &Value,
) -> Result<(), String> {
    let expected = schema
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("MCP tool {tool} argument {field} has no declared type"))?;
    let valid = match expected {
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "number" => value.is_number(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        other => {
            return Err(format!(
                "MCP tool {tool} argument {field} declares unsupported type {other}"
            ))
        }
    };
    if !valid {
        return Err(format!("{tool} argument {field} must be {expected}"));
    }
    if let (Some(items), Some(values)) = (schema.get("items"), value.as_array()) {
        for (index, value) in values.iter().enumerate() {
            validate_argument_type(tool, &format!("{field}[{index}]"), items, value)?;
        }
    }
    Ok(())
}

/// Serve the MCP protocol on stdio until the client closes it.
pub(crate) fn serve() {
    let tools = [TOOLS_JSON, STADO_JOB_TOOLS_JSON]
        .iter()
        .map(|catalogue| serde_json::from_str::<Vec<Value>>(catalogue))
        .collect::<Result<Vec<_>, _>>();
    let tools = match tools {
        Ok(parts) => Value::Array(parts.into_iter().flatten().collect()),
        Err(error) => {
            eprintln!("probierz mcp tool contract is invalid: {error}");
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
