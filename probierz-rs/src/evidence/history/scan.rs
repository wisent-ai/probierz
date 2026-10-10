//! `probierz secrets scan`: the report, then a refusal when the scan found
//! anything. The refusal is returned rather than ending the process, so an
//! in-process caller such as the MCP server keeps serving later requests.

use crate::evidence::*;

pub fn secrets_scan_command(root: Option<&Path>) -> Answer {
    let root = root.ok_or_else(|| {
        Failure::invalid("evidence.secret_scan", "secrets scan needs a directory")
    })?;
    let result = scan_secrets(root)?;
    print_json(&result)?;
    if result["passed"] != Value::Bool(true) {
        let found = match result["findings"].as_array() {
            Some(findings) => findings.len().to_string(),
            None => format!("an unreadable findings list ({})", result["findings"]),
        };
        return Err(Failure::invalid(
            "evidence.secret_scan",
            format!(
                "secret scan of {} found {found} finding(s); the report above lists them",
                root.display()
            ),
        ));
    }
    Ok(())
}
