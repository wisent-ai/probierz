use crate::gate::*;
pub fn prepush(harness: &Path, args: &PrepushArgs) -> Answer {
    let repo = args.repo.clone().unwrap_or(std::env::current_dir()?);
    let mut base = args.base.clone();
    let mut head = args.head.clone();
    if args.hook {
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input)?;
        let Some((hook_base, hook_head)) = parse_hook_refs(&input) else {
            println!("prepush-gate: push does not target main; allowed");
            return Ok(());
        };
        base = if hook_base.is_empty() {
            None
        } else {
            Some(hook_base)
        };
        head = Some(hook_head);
    }
    let result = prepush_value(
        harness,
        &repo,
        args.app_id.as_deref(),
        base.as_deref(),
        head.as_deref(),
        args.run_ci,
        &args.ci_args,
    )?;
    if args.hook && !args.json {
        let ok = truthy(property(&result, "ok"));
        let app_id = string_property(&result, "appId").unwrap_or_default();
        let note = string_property(&result, "note")
            .map(|note| format!(" ({note})"))
            .unwrap_or_default();
        println!(
            "prepush-gate {app_id}: {}{note}",
            if ok { "ALLOWED" } else { "BLOCKED" }
        );
        for error in
            value_strings(property(&result, "verdict").and_then(|value| property(value, "errors")))
        {
            println!("  - {error}");
        }
        if let Some(reason) = string_property(&result, "reason") {
            println!("  - {reason}");
        }
    } else {
        print_json(&result)?;
    }
    if !truthy(property(&result, "ok")) {
        std::process::exit(1);
    }
    Ok(())
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub(crate) fn present(file: &Path) -> bool {
    fs::symlink_metadata(file).is_ok()
}

pub(crate) fn managed(file: &Path, legacy_command: &str, rust_command: &str) -> bool {
    fs::read_to_string(file)
        .map(|content| {
            content.contains(MANAGED_MARKER)
                || (content.contains(legacy_command) && content.contains("--hook --app"))
                || (content.contains(rust_command)
                    && content.contains("gate-prepush")
                    && content.contains("--hook"))
        })
        .unwrap_or(false)
}

pub fn install(harness: &Path, args: &InstallArgs) -> Answer {
    manifest::load(harness, &args.app_id)?;
    let repo = args.repo.clone().unwrap_or(std::env::current_dir()?);
    let hooks = repo.join(".git").join("hooks");
    if !hooks.exists() {
        return Err(Failure::config(
            "gate.install",
            format!("not a git working tree: {}", repo.display()),
        ));
    }
    let target = hooks.join("pre-push");
    let backup = hooks.join("pre-push.before-probierz-gate");
    let executable = std::env::current_exe()?;
    let rust_command = executable.to_string_lossy().into_owned();
    let legacy_command = harness
        .join("agent")
        .join("prepush-gate.mjs")
        .to_string_lossy()
        .into_owned();
    if present(&backup) && managed(&backup, &legacy_command, &rust_command) {
        fs::remove_file(&backup)?;
    }
    if present(&target) && !present(&backup) && !managed(&target, &legacy_command, &rust_command) {
        fs::rename(&target, &backup)?;
    }
    let script = format!(
        "#!/bin/sh\n{MANAGED_MARKER}\nHOOK_DIR=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\nif [ -f \"$HOOK_DIR/pre-push.before-probierz-gate\" ]; then\n  \"$HOOK_DIR/pre-push.before-probierz-gate\" \"$@\" || exit $?\nfi\nGATE_CI=\"--ci\"\nif [ \"${{PROBIERZ_GATE_NO_CI:-}}\" = \"1\" ]; then GATE_CI=\"\"; fi\nexec {} --harness {} gate-prepush --hook --app {} $GATE_CI\n",
        shell_quote(&rust_command),
        shell_quote(&harness.to_string_lossy()),
        shell_quote(&args.app_id),
    );
    fs::create_dir_all(&hooks)?;
    fs::write(&target, script)?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
    print_json(&object([
        (
            "installed",
            Value::String(target.to_string_lossy().into_owned()),
        ),
        ("chained", Value::Bool(backup.exists())),
        ("appId", Value::String(args.app_id.clone())),
    ]))
}
