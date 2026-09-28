//! One Weles browser, driven through `weles mcp`.
//!
//! Browsers belong to Weles: a web journey never links a browser library. It
//! starts `weles mcp` (JSON-RPC 2.0, one message per line on stdin/stdout),
//! opens one browser context, and drives pages by the ids Weles hands out.
//! The operations and their answers are the ones documented on
//! weles.wisent.com/docs/mcp-sessions.

use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};

use serde_json::json;

use crate::specs::*;

mod constants;
mod expect;
mod page;

pub(crate) use expect::{label, role, role_exact, text, text_exact};
pub(crate) use page::Page;

struct Session {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next_id: u64,
}

impl Session {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.input, "{line}")
            .and_then(|()| self.input.flush())
            .map_err(|error| format!("weles mcp stopped reading requests: {error}"))?;
        loop {
            let mut reply = String::new();
            let read = self
                .output
                .read_line(&mut reply)
                .map_err(|error| format!("weles mcp answer could not be read: {error}"))?;
            if read == 0 {
                return Err(format!("weles mcp exited before answering {method}"));
            }
            let message: Value = serde_json::from_str(reply.trim()).map_err(|error| {
                format!(
                    "weles mcp wrote a line that is not JSON-RPC ({error}): {}",
                    reply.trim()
                )
            })?;
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(format!("weles refused {method}: {error}"));
            }
            return message
                .get("result")
                .cloned()
                .ok_or_else(|| format!("weles mcp answered {method} without a result"));
        }
    }

    /// A tool's answer: the text of its first content item.
    fn tool(&mut self, name: &str, arguments: Value) -> Result<String, String> {
        let result = self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )?;
        result
            .pointer("/content/0/text")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("weles answered {name} without text content: {result}"))
    }
}

pub(crate) struct Weles {
    session: Mutex<Session>,
    browser: String,
}

impl Weles {
    /// Start `weles mcp` (WELES_BIN, or `weles` on PATH) and one browser in it.
    /// PROBIERZ_HEADED=1 shows the browser window.
    pub(crate) fn start(context: &Context) -> Result<Weles, String> {
        let program = context
            .optional("WELES_BIN")
            .unwrap_or_else(|| "weles".to_string());
        let mut child = Command::new(&program)
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| {
                format!("{program} mcp could not start ({error}); web journeys drive pages only through Weles: install weles or set WELES_BIN")
            })?;
        let input = child.stdin.take().ok_or("weles mcp has no stdin")?;
        let output = BufReader::new(child.stdout.take().ok_or("weles mcp has no stdout")?);
        let mut session = Session {
            child,
            input,
            output,
            next_id: u64::MIN,
        };
        let client = json!({ "name": "probierz", "version": env!("CARGO_PKG_VERSION") });
        session.request(
            "initialize",
            json!({ "protocolVersion": constants::MCP_PROTOCOL, "capabilities": {}, "clientInfo": client }),
        )?;
        let headless = context.optional("PROBIERZ_HEADED").as_deref() != Some("1");
        let started =
            parse(&session.tool("weles_browser_start", json!({ "headless": headless }))?)?;
        let browser = started
            .get("browserId")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("weles_browser_start answered no browserId: {started}"))?
            .to_string();
        Ok(Weles {
            session: Mutex::new(session),
            browser,
        })
    }

    /// Call a tool and read its answer as JSON.
    pub(crate) fn call(&self, name: &str, arguments: Value) -> Result<Value, String> {
        parse(&self.call_text(name, arguments)?)
    }

    /// Call a tool whose answer is plain text (weles_page_text).
    pub(crate) fn call_text(&self, name: &str, arguments: Value) -> Result<String, String> {
        self.session
            .lock()
            .map_err(|_| "the weles session lock is poisoned".to_string())?
            .tool(name, arguments)
    }

    /// A new page; relative addresses resolve against `base`.
    pub(crate) fn page(&self, base: Option<url::Url>) -> Result<Page<'_>, String> {
        let opened = self.call("weles_page_new", json!({ "browserId": self.browser }))?;
        let id = opened
            .get("pageId")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("weles_page_new answered no pageId: {opened}"))?;
        Ok(Page::new(self, id.to_string(), base))
    }

    /// Headers every later request of this browser carries.
    pub(crate) fn headers(&self, headers: Value) -> Result<(), String> {
        self.call(
            "weles_browser_headers",
            json!({ "browserId": self.browser, "headers": headers }),
        )
        .map(drop)
    }

    /// Start signed in: a Playwright storage-state file the operator exported.
    pub(crate) fn storage(&self, path: &Path) -> Result<(), String> {
        self.call(
            "weles_browser_storage",
            json!({ "browserId": self.browser, "path": path }),
        )
        .map(drop)
    }
}

impl Drop for Weles {
    fn drop(&mut self) {
        if let Ok(mut session) = self.session.lock() {
            let _ = session.tool("weles_browser_close", json!({ "browserId": self.browser }));
            let _ = session.child.kill();
            let _ = session.child.wait();
        }
    }
}

fn parse(text: &str) -> Result<Value, String> {
    serde_json::from_str(text)
        .map_err(|error| format!("weles answered text that is not JSON ({error}): {text}"))
}
