use crate::{
    specs::{self, tui::common},
    tui::{Spawn, Terminal},
};
use std::{path::PathBuf, time::Duration};
fn command(
    app: &mut Terminal,
    number: usize,
    text: &str,
    expected: Option<i32>,
) -> Result<String, String> {
    let marker = format!("__GAC_TUI_CMD_{number}_DONE__");
    let start = app.full_log().len();
    app.send(&format!("{text}; printf \\\"\\n{marker} exit=$?\\n\\\""))
        .map_err(|e| e.detail)?;
    app.key("enter").map_err(|e| e.detail)?;
    app.wait_for(&marker, Duration::from_secs(30), true)
        .map_err(|e| e.detail)?;
    let output = app.full_log().get(start..).unwrap_or("").to_string();
    let re = regex::Regex::new(&format!(r"{} exit=(\d+)", regex::escape(&marker))).unwrap();
    let code = re.captures(&output).and_then(|c| c[1].parse().ok());
    if let Some(want) = expected {
        if code != Some(want) {
            return Err(format!("expected exit {want} from: {text}\n{output}"));
        }
    }
    Ok(output.split(&marker).next().unwrap_or("").to_string())
}
fn object(output: &str) -> Result<serde_json::Value, String> {
    let start = output
        .find('{')
        .ok_or_else(|| format!("expected JSON in output:\n{output}"))?;
    for (end, ch) in output[start..].char_indices() {
        if ch == '}' {
            if let Ok(value) = serde_json::from_str(&output[start..=start + end]) {
                return Ok(value);
            }
        }
    }
    Err(format!("unbalanced JSON in output:\n{output}"))
}
pub fn run(context: &specs::Context) -> Result<(), String> {
    let root = PathBuf::from(
        context
            .optional("GAC_ROOT")
            .unwrap_or_else(|| "/Users/lukaszbartoszcze/work/game_asset_creator".into()),
    );
    let fixtures = PathBuf::from(common::required(
        context,
        "GAC_FIXTURE_DIR",
        "GAC_FIXTURE_DIR is required: run the game-asset-creator seed capability",
    )?);
    for name in [
        "valid-6k.glb",
        "over-budget.glb",
        "corrupt.glb",
        "skarbiec",
        "pipeline.config.json",
    ] {
        if !fixtures.join(name).exists() {
            return Err(format!(
                "game asset creator fixture is required: {}",
                fixtures.join(name).display()
            ));
        }
    }
    let path = format!(
        "{}:{}",
        fixtures.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut app = Terminal::spawn(
        Spawn::new("/bin/sh")
            .args([
                "-c",
                "stty -echo; printf '__GAC_TUI_READY__\\n'; exec /bin/sh",
            ])
            .cwd(&root)
            .env("PATH", path)
            .env("SKARBIEC_BIN", fixtures.join("skarbiec").to_string_lossy()),
    )
    .map_err(|e| e.detail)?;
    let result = (|| {
        app.wait_for("__GAC_TUI_READY__", Duration::from_secs(30), true)
            .map_err(|e| e.detail)?;
        let config = fixtures.join("pipeline.config.json");
        let valid = fixtures.join("valid-6k.glb");
        let over = fixtures.join("over-budget.glb");
        let corrupt = fixtures.join("corrupt.glb");
        let checked = object(&command(
            &mut app,
            1,
            &format!(
                "node pipeline/cli.js check-config --config {}",
                config.display()
            ),
            Some(0),
        )?)?;
        if checked["credentials"] != "<resolved: ok>" || checked["browser"]["headless"] != true {
            return Err(format!("unexpected resolved config: {checked}"));
        }
        let report = object(&command(
            &mut app,
            2,
            &format!(
                "node pipeline/cli.js verify {} --config {}",
                valid.display(),
                config.display()
            ),
            Some(0),
        )?)?;
        if report["ok"] != true || report["stats"]["triangles"] != 6000 {
            return Err(format!("unexpected valid GLB report: {report}"));
        }
        let report = object(&command(
            &mut app,
            3,
            &format!(
                "node pipeline/cli.js verify {} --config {}",
                over.display(),
                config.display()
            ),
            Some(1),
        )?)?;
        if report["ok"] != false
            || !report["errors"].as_array().is_some_and(|a| {
                a.iter()
                    .any(|e| e.as_str().is_some_and(|s| s.contains("triangle budget")))
            })
        {
            return Err(format!(
                "over-budget report did not name triangle budget: {report}"
            ));
        }
        let corrupt_out = command(
            &mut app,
            4,
            &format!(
                "node pipeline/cli.js verify {} --config {}",
                corrupt.display(),
                config.display()
            ),
            Some(1),
        )?;
        if !regex::Regex::new("(?i:not a GLB|bad magic|too small)")
            .unwrap()
            .is_match(&corrupt_out)
        {
            return Err(format!("corrupt GLB diagnosis missing: {corrupt_out}"));
        }
        let mcp=command(&mut app,5,"printf '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\\n' | node pipeline/mcp.js",None)?;
        let line = mcp
            .lines()
            .rev()
            .find(|l| l.trim().starts_with('{') && l.contains("\"tools\""))
            .ok_or("MCP tools/list emitted no tools JSON")?;
        let tools: serde_json::Value =
            serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
        let mut names = tools["result"]["tools"]
            .as_array()
            .ok_or("MCP tools were not an array")?
            .iter()
            .filter_map(|v| v["name"].as_str())
            .collect::<Vec<_>>();
        names.sort();
        let expected = [
            "gac_blender_health",
            "gac_check_config",
            "gac_create_asset",
            "gac_sculpt",
            "gac_verify_asset",
            "gac_weles_tools",
        ];
        if names != expected {
            return Err(format!("unexpected MCP tools: {names:?}"));
        }
        let health = command(&mut app, 6, "node pipeline/cli.js blender-health", Some(1))?;
        if !regex::Regex::new("(?i:failed to start|healthy\"?:\\s*false|setup)")
            .unwrap()
            .is_match(&health)
        {
            return Err(format!("blender-health did not diagnose absence: {health}"));
        }
        Ok(())
    })();
    let _ = app.close();
    result
}
