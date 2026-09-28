//! The package under test, prepared and served: `npm ci` and `npm run build`
//! in its checkout (PROBIERZ_APP_SOURCE, which the Stado job fills with the
//! submitted source), then its own test server, whose ready line names the
//! address the journey opens.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Stdio};

use super::constants::{PREPARE_SECONDS, SERVER_PORT};
use crate::specs::tui::common;
use crate::specs::*;

/// The line tests/visual/server.mjs prints once it listens.
const READY: &str = "visual-showcase ready ";

pub(super) struct Server {
    child: Child,
    pub(super) base: url::Url,
}

fn prepare(source: &Path, args: &[&str]) -> Result<(), String> {
    let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
    let output = common::run(
        "npm",
        &args,
        Some(source),
        &BTreeMap::new(),
        &[],
        None,
        Duration::from_secs(PREPARE_SECONDS),
    )?;
    if output.code() != Some(0) {
        return Err(format!(
            "npm {} in {} exited {:?}:\n{}",
            args.join(" "),
            source.display(),
            output.code(),
            output.combined()
        ));
    }
    Ok(())
}

impl Server {
    pub(super) fn start(context: &Context) -> Result<Server, String> {
        let source = PathBuf::from(context.required(
            "PROBIERZ_APP_SOURCE",
            "the wisent-components checkout (probierz stado run web --node-source <checkout>)",
        )?);
        prepare(
            &source,
            &["ci", "--no-audit", "--no-fund", "--loglevel=error"],
        )?;
        prepare(&source, &["run", "build"])?;
        let mut child = Command::new("node")
            .arg("tests/visual/server.mjs")
            .current_dir(&source)
            .env("PORT", SERVER_PORT)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("node tests/visual/server.mjs could not start: {error}"))?;
        let stdout = child.stdout.take().ok_or("the test server has no stdout")?;
        let mut lines = BufReader::new(stdout).lines();
        let address = loop {
            match lines.next() {
                Some(Ok(line)) => {
                    if let Some(address) = line.strip_prefix(READY) {
                        break address.trim().to_string();
                    }
                }
                Some(Err(error)) => return Err(format!("reading the test server: {error}")),
                None => {
                    let status = child.wait().map_err(|error| error.to_string())?;
                    return Err(format!(
                        "the test server exited ({status}) before it listened"
                    ));
                }
            }
        };
        let base = url::Url::parse(&address)
            .map_err(|error| format!("the test server named {address}, not an address: {error}"))?;
        Ok(Server { child, base })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
