use crate::mcp::*;
pub(crate) fn route(name: &str, args: &Map<String, Value>) -> Result<Vec<String>, String> {
    let mut output = Vec::new();
    let mut positional: Vec<&str> = Vec::new();
    // A switch the command spells after its arguments (`matrix ... --plan`).
    let mut switches: Vec<&str> = Vec::new();
    // An array the CLI takes as one comma-separated value (`--runs a,b`).
    let mut comma_lists: Vec<&str> = Vec::new();
    // A field one command spells under its own flag name, where the shared
    // spelling below would name a flag that command does not declare.
    let mut own_flags: Vec<(&str, &str)> = Vec::new();
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
            "identity"
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
        "probierz_evaluate_figure" => "evaluate figure",
        "probierz_evaluate_seo" => "evaluate seo",
        "probierz_evaluate_talk" => "evaluate talk",
        "probierz_evaluate_security" => "evaluate security",
        "probierz_security_journal" => {
            positional.push("path");
            "journal"
        }
        "probierz_create_readme_gif" => {
            positional.push("input");
            own_flags.push(("output", "out"));
            "gif"
        }
        "probierz_affected" => "affected",
        "probierz_ci" => "ci",
        "probierz_history" => {
            positional.push("appId");
            "history"
        }
        "probierz_dashboard" => {
            positional.push("appId");
            "dashboard"
        }
        "probierz_matrix_plan" => {
            positional.extend(["appId", "profile"]);
            switches.push("--plan");
            "matrix"
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
            "secrets scan"
        }
        "probierz_audit" => {
            // `audit` takes the application as its positional argument and
            // the recorded run as `--run`, not `--app` and `--run-id`.
            positional.push("appId");
            own_flags.push(("runId", "run"));
            "audit"
        }
        "probierz_gate_status" => {
            positional.push("appId");
            "gate status"
        }
        "probierz_status" => {
            positional.push("appId");
            "status"
        }
        // `gate prepush` reads every argument as a flag: --repo, --app,
        // --base, --head, --ci.
        "probierz_gate_prepush" => "gate prepush",
        "probierz_author_spec" => {
            positional.extend(["appId", "journey"]);
            "author spec"
        }
        "probierz_repair" => {
            positional.push("appId");
            // `repair` declares the recorded run as `--run`, not `--run-id`.
            own_flags.push(("runId", "run"));
            "repair"
        }
        "probierz_author_manifest" => {
            positional.push("appId");
            "author manifest"
        }
        "probierz_stado_run" => {
            positional.push("target");
            "remote run"
        }
        "probierz_stado_collect" => {
            positional.push("jobId");
            "remote collect"
        }
        "probierz_stado_resume" => {
            positional.push("jobId");
            "remote resume"
        }
        "probierz_stado_evaluate_seo" => {
            positional.push("appId");
            "remote seo"
        }
        "probierz_gate_evaluate" => {
            positional.extend(["appId", "mode", "expectedHarnessSha"]);
            comma_lists.push("runs");
            "gate evaluate"
        }
        "probierz_gate_enforce" => {
            positional.extend(["appId", "mode", "expectedHarnessSha"]);
            comma_lists.push("runs");
            "gate enforce"
        }
        "probierz_gate_activate" => {
            positional.extend(["appId", "mode", "expectedHarnessSha"]);
            comma_lists.push("runs");
            "gate activate"
        }
        "probierz_compare_runs" => {
            positional.extend(["leftRunId", "rightRunId", "appId"]);
            "compare"
        }
        "probierz_create_receipt" => {
            positional.extend(["appId", "release", "expectedHarnessSha"]);
            comma_lists.push("runs");
            comma_lists.push("journeys");
            "receipt create"
        }
        "probierz_verify_receipt" => {
            positional.push("file");
            "receipt verify"
        }
        "probierz_create_publication_manifest" => {
            positional.extend(["receipt", "attemptId", "journeyId"]);
            "publication"
        }
        _ => return Err(format!("unknown tool: {name}")),
    };
    // A command of two words is a group and its verb (`stado run`).
    output.extend(command.split(' ').map(str::to_string));
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
            let environment = value
                .as_object()
                .ok_or_else(|| "env must be an object of variable names to text".to_string())?;
            for (name, value) in environment {
                let text = value
                    .as_str()
                    .ok_or_else(|| format!("env.{name} must be text, not {value}"))?;
                output.push(format!("{name}={text}"));
            }
            continue;
        }
        // `stado` jobs are watched unless the caller says otherwise; the CLI
        // spells that choice `--no-watch`, and has no `--watch` flag.
        if key == "watch" {
            if value == &Value::Bool(false) {
                output.push("--no-watch".to_string());
            }
            continue;
        }
        let own_flag = own_flags
            .iter()
            .find(|(field, _)| *field == key.as_str())
            .map(|(_, flag)| *flag);
        let cli_key = match own_flag {
            Some(flag) => flag,
            None => match key.as_str() {
                "referencePath" => "reference",
                "candidatePath" => "candidate",
                "slidesPath" => "slides",
                "scriptPath" => "script",
                "sourcePaths" => "source",
                "rubricPath" => "rubric",
                "outputPath" => "out",
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
                "cargoRelease" => "cargo-release",
                "appRepo" => "app-repo",
                "noRepair" => "no-repair",
                "startSeconds" => "start",
                "durationSeconds" => "duration",
                "framesPerSecond" => "fps",
                other => other,
            },
        };
        if comma_lists.contains(&key.as_str()) {
            let items = value
                .as_array()
                .ok_or_else(|| format!("{key} must be an array of strings"))?;
            let words = items
                .iter()
                .map(|item| non_empty(Some(item), key))
                .collect::<Result<Vec<_>, _>>()?;
            append_flag(&mut output, cli_key, &Value::String(words.join(",")));
            continue;
        }
        append_flag(&mut output, cli_key, value);
    }
    output.extend(switches.iter().map(|switch| switch.to_string()));
    Ok(output)
}
