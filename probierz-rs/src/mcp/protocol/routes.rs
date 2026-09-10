use crate::*;
pub(crate) fn route(name: &str, args: &Map<String, Value>) -> Result<Vec<String>, String> {
    let mut output = Vec::new();
    let mut positional: Vec<&str> = Vec::new();
    let command = match name {
        "probierz_list_surfaces" => "list",
        "probierz_list_specs" => {
            if args.contains_key("surface") {
                positional.push("surface");
            }
            "specs"
        }
        "probierz_describe_spec" => {
            positional.push("spec");
            "describe"
        }
        "probierz_run_command" => {
            positional.push("target");
            "cmd"
        }
        "probierz_source_identity" => {
            positional.push("appId");
            "source-identity"
        }
        "probierz_check" => {
            positional.push("target");
            "check"
        }
        "probierz_setup" => {
            positional.push("target");
            "setup"
        }
        "probierz_run" => {
            positional.push("target");
            "run"
        }
        "probierz_analyze" => {
            positional.push("reportPath");
            "analyze"
        }
        "probierz_evaluate_figure" => "figure-evaluate",
        "probierz_evaluate_seo" => "seo-evaluate",
        "probierz_create_readme_gif" => {
            positional.push("input");
            "readme-gif"
        }
        "probierz_affected" => "affected",
        "probierz_ci" => "ci",
        "probierz_history" => "history",
        "probierz_dashboard" => {
            positional.push("appId");
            "dashboard"
        }
        "probierz_matrix_plan" => {
            positional.extend(["appId", "profile"]);
            "matrix-plan"
        }
        "probierz_run_matrix" => {
            positional.extend(["appId", "profile"]);
            "matrix"
        }
        "probierz_protect_run" => {
            positional.extend(["appId", "runId"]);
            "protect"
        }
        "probierz_restore_bundle" => {
            positional.extend(["file", "destination"]);
            "restore"
        }
        "probierz_retention" => {
            positional.push("appId");
            "retention"
        }
        "probierz_secret_scan" => {
            positional.push("directory");
            "secret-scan"
        }
        "probierz_audit" => "audit",
        "probierz_gate_status" => {
            positional.push("appId");
            "gate-status"
        }
        "probierz_status" => {
            positional.push("appId");
            "status"
        }
        "probierz_gate_prepush" => {
            positional.push("repo");
            "gate-prepush"
        }
        "probierz_author_spec" => {
            positional.extend(["appId", "journey"]);
            "author-spec"
        }
        "probierz_repair" => {
            positional.push("appId");
            "repair"
        }
        "probierz_author_manifest" => {
            positional.push("appId");
            "author-manifest"
        }
        "probierz_stado_run" => {
            positional.push("target");
            "stado"
        }
        "probierz_stado_evaluate_seo" => "stado-seo",
        "probierz_gate_evaluate" => {
            positional.push("appId");
            "gate-evaluate"
        }
        "probierz_gate_enforce" => {
            positional.push("appId");
            "gate-enforce"
        }
        "probierz_gate_activate" => {
            positional.push("appId");
            "gate-activate"
        }
        "probierz_compare_runs" => {
            positional.extend(["leftRunId", "rightRunId"]);
            "compare"
        }
        "probierz_last_green" => "last-green",
        "probierz_create_receipt" => {
            positional.extend(["appId", "release"]);
            "receipt-create"
        }
        "probierz_verify_receipt" => {
            positional.push("file");
            "receipt-verify"
        }
        "probierz_create_publication_manifest" => "publication-create",
        _ => return Err(format!("unknown tool: {name}")),
    };
    output.push(command.to_string());
    for key in &positional {
        if let Some(value) = args.get(*key) {
            output.push(non_empty(Some(value), key)?.to_string());
        }
    }
    for (key, value) in args {
        if positional.contains(&key.as_str()) {
            continue;
        }
        if key == "env" {
            if let Some(environment) = value.as_object() {
                for (name, value) in environment {
                    let text = value
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| value.to_string());
                    output.push(format!("{name}={text}"));
                }
            }
            continue;
        }
        let cli_key = match key.as_str() {
            "referencePath" => "reference",
            "candidatePath" => "candidate",
            "rubricPath" => "rubric",
            "outputPath" | "output" => "out",
            "texPreamblePath" => "tex-preamble",
            "routerBaseUrl" => "router-url",
            "policyPath" => "policy",
            "briefPath" => "brief",
            "productionEvidencePath" => "production-evidence",
            "privateKeyFile" => "private-key-file",
            "repositories" => "repo",
            "withSpecs" => "specs",
            "appId" => "app",
            "baseRef" => "base",
            "leftRunId" => "left",
            "rightRunId" => "right",
            "runIds" => "run",
            "cargoRelease" => "cargo-release",
            "appRepo" => "app-repo",
            "noRepair" => "no-repair",
            "timeoutMs" => "timeout",
            "resourceWaitMs" => "resource-wait",
            "startSeconds" => "start",
            "durationSeconds" => "duration",
            "framesPerSecond" => "fps",
            other => other,
        };
        append_flag(&mut output, cli_key, value);
    }
    Ok(output)
}

