use std::fs;

use serde_json::{json, Value};

use crate::specs;

use super::fleet_fixture::{self as fixture, FleetFixture, FIXTURE_HOST, FIXTURE_PRODUCT};

pub fn run(context: &specs::Context) -> Result<(), String> {
    let source = fixture::source_identity()?;
    let mut fleet = FleetFixture::open(context, "stado-release-logs")?;
    let result = run_fixture(context, &source, &mut fleet);
    let close = fleet.close();
    result.and(close)
}

fn run_fixture(
    context: &specs::Context,
    source: &Value,
    fleet: &mut FleetFixture,
) -> Result<(), String> {
    let install_root = fleet.services_root.join(FIXTURE_PRODUCT);
    let release = fixture::fixture_release_control(
        &fleet.home,
        &fleet.state_dir,
        &fleet.logs_root,
        "0.2.27",
        &"a".repeat(64),
        &install_root,
    );
    fleet.registry(&fixture::fixture_registry(json!({}), Some(release))?)?;
    let err_path = fleet
        .logs_root
        .join(format!("{FIXTURE_PRODUCT}-0.2.27.err"));
    let out_path = fleet
        .logs_root
        .join(format!("{FIXTURE_PRODUCT}-0.2.27.out"));
    let mut err_lines = (1..=10)
        .map(|index| format!("line {index}: starting listener"))
        .collect::<Vec<_>>();
    err_lines.push("panic: missing config key 'router.upstream'".to_string());
    fs::write(&err_path, format!("{}\n", err_lines.join("\n")))
        .map_err(|error| format!("{}: {error}", err_path.display()))?;
    fs::write(&out_path, "").map_err(|error| format!("{}: {error}", out_path.display()))?;
    let err_bytes = fs::metadata(&err_path)
        .map_err(|error| error.to_string())?
        .len();

    let read = fleet.invoke_json(&[
        "release",
        "logs",
        FIXTURE_PRODUCT,
        "--target",
        FIXTURE_HOST,
        "--stream",
        "both",
        "--lines",
        "3",
        "--json",
    ])?;
    fixture::ensure(
        read.status == 0,
        format!("reading logs failed: {}", read.output),
    )?;
    fixture::ensure(
        read.json["product"] == FIXTURE_PRODUCT,
        format!("wrong product: {}", read.json),
    )?;
    fixture::ensure(
        read.json["target"] == FIXTURE_HOST,
        format!("wrong target: {}", read.json),
    )?;
    fixture::ensure(
        read.json["version"] == "0.2.27",
        format!("wrong desired version: {}", read.json),
    )?;
    let err_read = stream(&read.json, "err")?;
    fixture::ensure(
        err_read["state"] == "read",
        format!("stderr state is not read: {err_read}"),
    )?;
    fixture::ensure(
        err_read["path"].as_str() == Some(err_path.to_string_lossy().as_ref()),
        format!("stderr path is wrong: {err_read}"),
    )?;
    fixture::ensure(
        err_read["lines"] == json!(err_lines[err_lines.len() - 3..]),
        "the tail is not the last lines of the file",
    )?;
    fixture::ensure(
        err_read["bytes"] == err_bytes,
        "a tail must report the size of the WHOLE file beside it",
    )?;
    fixture::ensure(
        err_read["lines"]
            .as_array()
            .and_then(|lines| lines.last())
            .and_then(Value::as_str)
            .map(|line| line.contains("router.upstream"))
            .unwrap_or(false),
        "the reason the candidate died is missing",
    )?;
    let out_read = stream(&read.json, "out")?;
    fixture::ensure(
        out_read["state"] == "empty",
        "a present, empty log is not the same answer as a missing one",
    )?;
    fixture::ensure(
        out_read["bytes"] == 0 && out_read["lines"] == json!([]),
        format!("empty stdout report is wrong: {out_read}"),
    )?;

    let rendered = fleet.invoke(&[
        "release",
        "logs",
        FIXTURE_PRODUCT,
        "--target",
        FIXTURE_HOST,
        "--lines",
        "3",
    ])?;
    fixture::ensure(
        rendered.status == 0,
        format!("the human rendering failed: {}", rendered.output),
    )?;
    let err_index = rendered.output.find(err_path.to_string_lossy().as_ref());
    let out_index = rendered.output.find(out_path.to_string_lossy().as_ref());
    fixture::ensure(
        matches!((err_index, out_index), (Some(left), Some(right)) if left < right),
        "stderr must be rendered before stdout",
    )?;

    let missing = fleet.invoke_json(&[
        "release",
        "logs",
        FIXTURE_PRODUCT,
        "--target",
        FIXTURE_HOST,
        "--version",
        "9.9.9",
        "--json",
    ])?;
    fixture::ensure(
        missing.status == 0,
        format!("reading a missing version failed: {}", missing.output),
    )?;
    fixture::ensure(
        missing.json["version"] == "9.9.9",
        format!("wrong missing version: {}", missing.json),
    )?;
    for name in ["err", "out"] {
        let entry = stream(&missing.json, name)?;
        fixture::ensure(
            entry["state"] == "missing",
            format!("{name} state is not missing: {entry}"),
        )?;
        fixture::ensure(
            entry["bytes"].is_null(),
            "a missing log has no size, not a size of zero",
        )?;
        fixture::ensure(
            entry["lines"] == json!([]),
            format!("missing {name} has lines: {entry}"),
        )?;
    }
    let only = fleet.invoke_json(&[
        "release",
        "logs",
        FIXTURE_PRODUCT,
        "--target",
        FIXTURE_HOST,
        "--stream",
        "err",
        "--json",
    ])?;
    fixture::ensure(
        only.status == 0,
        format!("one-stream read failed: {}", only.output),
    )?;
    fixture::ensure(
        only.json["streams"].as_array().map(|rows| {
            rows.iter()
                .map(|row| row["stream"].clone())
                .collect::<Vec<_>>()
        }) == Some(vec![json!("err")]),
        format!("one-stream read returned the wrong streams: {}", only.json),
    )?;
    let refused = fleet.invoke(&[
        "release",
        "logs",
        FIXTURE_PRODUCT,
        "--target",
        FIXTURE_HOST,
        "--lines",
        "0",
    ])?;
    fixture::ensure(refused.status != 0, "--lines 0 must be refused")?;
    fixture::ensure(
        refused.output.contains("--lines must be at least 1"),
        format!("--lines 0 refusal is wrong: {}", refused.output),
    )?;
    fixture::ensure(
        fs::read_to_string(&err_path).map_err(|error| error.to_string())?
            == format!("{}\n", err_lines.join("\n")),
        "reading logs changed stderr",
    )?;
    fixture::ensure(
        fs::metadata(&out_path)
            .map_err(|error| error.to_string())?
            .len()
            == 0,
        "reading logs changed stdout",
    )?;

    fixture::record_trace(
        context,
        "stado-release-logs",
        "release-logs",
        &fleet.binary,
        source.clone(),
        json!({
            "readState":{"state":err_read["state"],"bytes":err_read["bytes"],"lines":err_read["lines"].as_array().map(Vec::len).unwrap_or(0)},
            "emptyState":{"state":out_read["state"],"bytes":out_read["bytes"]},
            "missingState":stream(&missing.json,"err")?["state"],
            "refusedExitStatus":refused.status
        }),
        &[
            "a log that was read reports its last lines and the size of the whole file",
            "a present, empty log is reported as empty, not as missing",
            "a log the agent never wrote is reported as missing, with no size",
            "stderr is read and rendered before stdout",
            "--lines 0 is refused instead of answered with an empty tail",
            "reading a candidate log changes nothing on the host",
        ],
    )
}

fn stream<'a>(report: &'a Value, name: &str) -> Result<&'a Value, String> {
    report["streams"]
        .as_array()
        .and_then(|streams| streams.iter().find(|entry| entry["stream"] == name))
        .ok_or_else(|| format!("the report carries no {name} stream"))
}
