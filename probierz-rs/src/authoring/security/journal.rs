//! Append-only progress survives interruption before the final assessment report.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum State {
    Started,
    Observed,
    Failed,
    Finished,
}

#[derive(Deserialize, Serialize)]
struct Event {
    kind: String,
    at: String,
    operation: String,
    check: Option<String>,
    state: State,
    detail: Value,
}

pub struct Journal<'a> {
    file: File,
    write_failed: bool,
    pub path: PathBuf,
    pub operation: &'static str,
    pub check: Option<&'a str>,
}

impl<'a> Journal<'a> {
    pub fn create(report: &Path, app: &str, identity: &Value) -> Result<Self, String> {
        let mut path = report.as_os_str().to_os_string();
        path.push(".events.jsonl");
        let path = PathBuf::from(path);
        let file = File::create_new(&path)
            .map_err(|error| format!("create security journal {}: {error}", path.display()))?;
        let mut journal = Self {
            file,
            write_failed: false,
            path,
            operation: "initialize",
            check: None,
        };
        journal.append(
            State::Started,
            &json!({"appId": app, "sourceIdentity": identity}),
        )?;
        Ok(journal)
    }

    fn append(&mut self, state: State, detail: &Value) -> Result<(), String> {
        if self.write_failed {
            return Err(format!("journal {} is unavailable after a persistence failure; no later event was appended", self.path.display()));
        }
        let event = json!({"kind": "probierz-security-event", "at": chrono::Utc::now(),
            "operation": self.operation, "check": self.check, "state": state, "detail": detail});
        let result = (|| {
            serde_json::to_writer(&mut self.file, &event).map_err(|error| {
                format!("write security journal {}: {error}", self.path.display())
            })?;
            self.file
                .write_all(b"\n")
                .and_then(|()| self.file.sync_all())
                .map_err(|error| {
                    format!("persist security journal {}: {error}", self.path.display())
                })
        })();
        self.write_failed = result.is_err();
        result
    }

    pub fn begin(&mut self, operation: &'static str, check: Option<&'a str>) -> Result<(), String> {
        self.operation = operation;
        self.check = check;
        self.append(State::Started, &Value::Null)
    }

    pub fn observed(&mut self, observation: &Value) -> Result<(), String> {
        self.append(State::Observed, observation)
    }

    pub fn finish(&mut self, error: Option<&str>) -> Result<(), String> {
        if error.is_none() {
            self.operation = "complete";
            self.check = None;
        }
        self.append(
            if error.is_some() {
                State::Failed
            } else {
                State::Finished
            },
            &json!({"error": error}),
        )
    }
}

/// Inspect progress without contacting a target or model; never return a security pass.
pub fn inspect_security_journal(path: &Path) -> Result<Value, crate::authoring::Failure> {
    let refuse = |message: String| {
        crate::authoring::Failure::config(
            "security-journal",
            format!("{}: {message}", path.display()),
        )
    };
    let bytes = std::fs::read(path).map_err(|error| refuse(format!("read journal: {error}")))?;
    let mut events: Vec<Event> = Vec::new();
    let mut partial = false;
    let mut terminal = false;
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        let Some(record) = line.strip_suffix(b"\n") else {
            partial = true;
            break;
        };
        if terminal {
            return Err(refuse("events follow a terminal journal record".into()));
        }
        let event: Event = serde_json::from_slice(record)
            .map_err(|error| refuse(format!("invalid complete journal record: {error}")))?;
        if event.kind != "probierz-security-event" || event.operation.is_empty() {
            return Err(refuse("invalid security journal event".into()));
        }
        if events.is_empty()
            && (event.operation != "initialize"
                || event.state != State::Started
                || event.detail["appId"].as_str().is_none())
        {
            return Err(refuse(
                "journal must begin with a complete initialization record".into(),
            ));
        }
        if event.state == State::Observed
            && (event.operation != "check"
                || event.check.is_none()
                || event.detail["path"].as_str().is_none()
                || event.detail["status"].as_i64().is_none()
                || event.detail["pass"].as_bool().is_none())
        {
            return Err(refuse(
                "observation lacks its check, path, status or verdict".into(),
            ));
        }
        terminal = event.state == State::Failed || event.state == State::Finished;
        events.push(event);
    }
    if events.is_empty() {
        return Err(refuse(
            "journal has no complete initialization record".into(),
        ));
    }
    Ok(json!({"kind": "probierz-security-journal", "journal": path,
        "partialRecord": partial, "events": events,
        "notice": "Execution progress is not a final security assessment or proof of a passing result."}))
}
