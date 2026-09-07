//! The skill document names commands, targets and tools. It kept naming
//! implementation files that no longer existed, so it is generated now.
//!
//! Two tables in `skills/probierz/SKILL.md` are produced from the binaries
//! themselves: the CLI command surface from `probierz --help`, and the MCP tool
//! surface from a real `tools/list` call to `probierz-mcp`. This test fails
//! when the document and the binaries disagree, and rewrites the document when
//! run with `SKILL_DOC_WRITE=1`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const CLI_BEGIN: &str = "<!-- generated: cli -->";
const CLI_END: &str = "<!-- /generated: cli -->";
const MCP_BEGIN: &str = "<!-- generated: mcp -->";
const MCP_END: &str = "<!-- /generated: mcp -->";

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("probierz-rs sits in the repository")
        .to_path_buf()
}

fn help_of(binary: &str, argument: Option<&str>) -> String {
    let mut command = Command::new(binary);
    if let Some(argument) = argument {
        command.arg(argument);
    }
    let output = command
        .arg("--help")
        .output()
        .expect("the binary answers --help");
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Every command in the top-level help, with the sentence it describes itself
/// by. Clap prints `  name  description`, wrapping long descriptions onto
/// indented continuation lines.
fn cli_commands(binary: &str) -> Vec<(String, String)> {
    let help = help_of(binary, None);
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut inside = false;
    for line in help.lines() {
        if line.starts_with("Commands:") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if line.trim().is_empty() {
            break;
        }
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let mut parts = trimmed.splitn(2, "  ");
        let name = parts.next().unwrap_or("").trim().to_string();
        let text = parts.next().unwrap_or("").trim().to_string();
        if indent <= 4 && !name.is_empty() && name != "help" {
            rows.push((name, text));
        } else if let Some(last) = rows.last_mut() {
            last.1 = format!("{} {}", last.1, trimmed.trim()).trim().to_string();
        }
    }
    rows
}

fn cli_table(binary: &str) -> String {
    let mut lines = vec![
        format!("Every command of `probierz {}`:", version(binary)),
        String::new(),
        "|Command|What it does|".to_string(),
        "|---|---|".to_string(),
    ];
    for (name, text) in cli_commands(binary) {
        let text = if text.is_empty() {
            let own = help_of(binary, Some(&name));
            own.lines().next().unwrap_or("").trim().to_string()
        } else {
            text
        };
        lines.push(format!("|`{name}`|{text}|"));
    }
    lines.join("\n")
}

fn version(binary: &str) -> String {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .expect("--version");
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .last()
        .unwrap_or("")
        .to_string()
}

/// The tools the MCP server advertises, asked over the protocol it serves.
fn mcp_table(server: &str) -> String {
    let mut child = Command::new(server)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the MCP server starts");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\",\"params\":{}}\n")
        .expect("the request is written");
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("the server answers");
    let body = String::from_utf8_lossy(&output.stdout);
    let frame = body
        .lines()
        .find(|line| line.contains("\"tools\""))
        .unwrap_or_else(|| panic!("tools/list produced no tool frame: {body}"));
    let answer: serde_json::Value = serde_json::from_str(frame).expect("the frame is JSON-RPC");
    let tools = answer["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("tools/list has no tools array: {frame}"));
    let mut lines = vec![
        "Every tool `probierz-mcp` advertises over `tools/list`:".to_string(),
        String::new(),
        "|Tool|What it does|".to_string(),
        "|---|---|".to_string(),
    ];
    for tool in tools {
        let name = tool["name"].as_str().unwrap_or_default();
        let text = tool["description"].as_str().unwrap_or_default();
        let text = text.split('\n').next().unwrap_or_default().trim();
        lines.push(format!("|`{name}`|{text}|"));
    }
    lines.join("\n")
}

fn replace_between(document: &str, begin: &str, end: &str, body: &str) -> String {
    let start = document
        .find(begin)
        .unwrap_or_else(|| panic!("SKILL.md is missing the {begin} marker"));
    let finish = document
        .find(end)
        .unwrap_or_else(|| panic!("SKILL.md is missing the {end} marker"));
    let head = &document[..start + begin.len()];
    let tail = &document[finish..];
    format!("{head}\n\n{body}\n\n{tail}")
}

#[test]
fn the_skill_document_names_exactly_what_the_binaries_answer() {
    let root = repository();
    let cli = root.join("probierz-rs/target/debug/probierz");
    let server = root.join("probierz-rs/target/debug/probierz-mcp");
    assert!(cli.exists(), "build the product first: cargo build --bins");
    assert!(
        server.exists(),
        "build the product first: cargo build --bins"
    );

    let path = root.join("skills/probierz/SKILL.md");
    let document = std::fs::read_to_string(&path).expect("the skill document is readable");
    let updated = replace_between(
        &document,
        CLI_BEGIN,
        CLI_END,
        &cli_table(cli.to_string_lossy().as_ref()),
    );
    let updated = replace_between(
        &updated,
        MCP_BEGIN,
        MCP_END,
        &mcp_table(server.to_string_lossy().as_ref()),
    );

    if std::env::var_os("SKILL_DOC_WRITE").is_some() {
        std::fs::write(&path, &updated).expect("the skill document is writable");
        return;
    }
    assert_eq!(
        document, updated,
        "skills/probierz/SKILL.md disagrees with the binaries; regenerate it with SKILL_DOC_WRITE=1 cargo test --test skill_doc"
    );
}
