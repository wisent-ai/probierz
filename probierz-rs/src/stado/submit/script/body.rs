use crate::stado::*;
#[allow(clippy::too_many_arguments)]
pub(crate) fn script_body(
    target: &str,
    app_id: &str,
    hash: &str,
    spec: Option<&str>,
    provision: Option<&Provision>,
    mode: &str,
    author: Option<(&str, &str, &str, &str)>,
    model_router_url: Option<&str>,
    record: bool,
    environment: &[(String, String)],
    lines: &mut Vec<String>,
) -> Result<(), Failure> {
    lines.extend([
        "cd \"$HARNESS\"".into(),
        "export PROBIERZ_SOURCE_IDENTITY=\"$JOB_ROOT/inputs/source-identity.json\"".into(),
    ]);
    if mode == "author" || (mode != "run" && provision.is_some()) {
        let source = match provision {
            Some(Provision::AppBundle { app_id, .. }) => format!("$JOB_ROOT/work/{app_id}-src"),
            Some(value) => format!("$JOB_ROOT/work/{}", value.app_id()),
            None => format!("$JOB_ROOT/work/{app_id}"),
        };
        lines.push(format!(
            "perl -pi -e \"s|^  - root: .*|  - root: {source}|\" apps/{app_id}/probierz.yaml"
        ));
    }
    if matches!(
        target,
        "mobile:ios" | "mobile:android" | "desktop:mac" | "desktop:win"
    ) {
        let appium = if target == "desktop:mac" {
            "appium-2-mac2-2.2.2"
        } else {
            "appium-2"
        };
        lines.push(format!(
            "export APPIUM_HOME=\"$HOME/.cache/probierz/{appium}\""
        ));
    }
    lines.push("npm ci --no-audit --no-fund --loglevel=error".into());
    if target != "tui" {
        lines.push(format!(
            "\"$PROBIERZ\" --harness \"$HARNESS\" setup {target}"
        ));
    }
    if mode == "script" {
        let script = match provision {
            Some(Provision::NodeSource {
                script: Some(script),
                ..
            }) => script,
            _ => {
                return Err(Failure::config(
                    "stado.submit",
                    "script mode needs a staged node-source script",
                ))
            }
        };
        lines.extend([
            "mkdir -p test-results".into(),
            "set +e".into(),
            format!("bash apps/{app_id}/{script}"),
            "PROBIERZ_RUN_RC=$?".into(),
            "set -e".into(),
            format!("tar -czf \"$JOB_ROOT/output/probierz-run-{hash}.tar.gz\" test-results"),
            "exit $PROBIERZ_RUN_RC".into(),
        ]);
        return Ok(());
    }
    if mode == "author" {
        let (journey, area, description, receipt_id) = author.ok_or_else(|| {
            Failure::config(
                "stado.submit",
                "remote authoring needs its journey contract",
            )
        })?;
        let router = model_router_url
            .ok_or_else(|| Failure::config("stado.submit", "STADO_MODEL_ROUTER_URL is required"))?;
        lines.extend([
            format!("export STADO_MODEL_ROUTER_URL={}", shell_quote(router)),
            ": \"${STADO_MODEL_ROUTER_TOKEN:?STADO_MODEL_ROUTER_TOKEN was not materialized by Stado}\"".into(),
            "export PROBIERZ_MODEL_AGENT_ID=probierz".into(),
            ": \"${PROBIERZ_MODEL_AGENT_SECRET:?PROBIERZ_MODEL_AGENT_SECRET was not materialized by Stado}\"".into(),
            format!("export PROBIERZ_AUTHOR_RECEIPT_ID={}", shell_quote(receipt_id)),
        ]);
        if matches!(
            target,
            "mobile:ios" | "mobile:android" | "desktop:mac" | "desktop:win"
        ) {
            lines.extend([
                "pkill -f '[a]ppium.*--port 4723' >/dev/null 2>&1 || true".into(),
                "npx appium --relaxed-security --port 4723 > /tmp/appium.log 2>&1 &".into(),
                "APPIUM_PID=$!".into(),
                "trap 'kill \"$APPIUM_PID\" >/dev/null 2>&1 || true' EXIT".into(),
                "export PROBIERZ_EXTERNAL_APPIUM=1".into(),
                "for i in $(seq 1 30); do nc -z 127.0.0.1 4723 && break; sleep 2; done".into(),
                "nc -z 127.0.0.1 4723".into(),
            ]);
        }
        let app_path = if target == "web" {
            String::new()
        } else if target == "tui" {
            " --app-path \"$TUI_CMD\"".into()
        } else {
            " --app-path \"$MAC_APP_PATH\"".into()
        };
        lines.extend([
            "set +e".into(),
            format!(
                "\"$PROBIERZ\" --harness \"$HARNESS\" author-spec {} {} --area {} --target {} --desc {}{app_path} > \"$JOB_ROOT/work/author-result.json\"",
                shell_quote(app_id), shell_quote(journey), shell_quote(area), shell_quote(target), shell_quote(description),
            ),
            "PROBIERZ_RC=$?".into(),
            "set -e".into(),
            "if [ \"$PROBIERZ_RC\" -eq 0 ]; then".into(),
            format!(
                "  \"$PROBIERZ\" --harness \"$HARNESS\" stado author-receipt --app {} --journey {} --area {} --target {} --receipt-id {} --result \"$JOB_ROOT/work/author-result.json\"",
                shell_quote(app_id), shell_quote(journey), shell_quote(area), shell_quote(target), shell_quote(receipt_id),
            ),
            "fi".into(),
            "mkdir -p test-results".into(),
            format!("tar -czf \"$JOB_ROOT/output/probierz-author-{hash}.tar.gz\" test-results"),
            "exit $PROBIERZ_RC".into(),
        ]);
        return Ok(());
    }
    let has_source = matches!(
        provision,
        Some(
            Provision::AppBundle { .. }
                | Provision::CargoRelease { .. }
                | Provision::NativeBinary { .. }
                | Provision::NodeSource { .. }
        )
    );
    let mut conditions = vec!["PROBIERZ_RUN_KIND=pull-request".to_string()];
    if target == "tui" && !matches!(provision, Some(Provision::NodeSource { .. })) {
        conditions.push("TUI_CMD=\"$TUI_CMD\"".into());
    }
    if has_source {
        conditions.push("PROBIERZ_APP_SOURCE=\"$PROBIERZ_APP_SOURCE\"".into());
    }
    if target == "desktop:cua" {
        conditions.push("CUA_APP_EXECUTABLE=\"$CUA_APP_EXECUTABLE\"".into());
    }
    conditions.extend(
        environment
            .iter()
            .map(|(name, value)| shell_quote(&format!("{name}={value}"))),
    );
    let source_flag = if has_source {
        " --app-repo \"$PROBIERZ_APP_SOURCE\""
    } else {
        ""
    };
    let spec_flag = spec
        .map(|value| format!(" --spec {value}"))
        .unwrap_or_default();
    let record_flag = if record { " --record" } else { "" };
    lines.extend([
        "set +e".into(),
        format!("\"$PROBIERZ\" --harness \"$HARNESS\" run {target} --app {app_id}{source_flag}{spec_flag}{record_flag} {}", conditions.join(" ")),
        "PROBIERZ_RUN_RC=$?".into(),
        "set -e".into(),
        format!("tar -czf \"$JOB_ROOT/output/probierz-run-{hash}.tar.gz\" test-results"),
        "exit $PROBIERZ_RUN_RC".into(),
    ]);
    Ok(())
}
