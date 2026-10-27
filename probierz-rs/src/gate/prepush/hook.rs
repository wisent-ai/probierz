use crate::gate::*;
use crate::paths::present;
use std::path::PathBuf;

pub fn prepush(harness: &Path, args: &PrepushArgs) -> Answer {
    let repo = repository(&args.repo)?;
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
        let ok = passed(&result);
        let app_id = string_property(&result, "appId")
            .ok_or_else(|| Failure::invalid("gate.prepush", format!("the gate answered no appId: {result}")))?;
        let note = match string_property(&result, "note") {
            Some(note) => format!(" ({note})"),
            None => String::new(),
        };
        println!(
            "prepush-gate {app_id}: {}{note}",
            if ok { "ALLOWED" } else { "BLOCKED" }
        );
        if let Some(verdict) = property(&result, "verdict") {
            for error in value_strings(property(verdict, "errors"), "prepush verdict.errors")
                .map_err(|detail| Failure::config("gate.prepush", detail))?
            {
                println!("  - {error}");
            }
        }
        if let Some(reason) = string_property(&result, "reason") {
            println!("  - {reason}");
        }
    } else {
        print_json(&result)?;
    }
    if !passed(&result) {
        return Err(Failure::new(
            "gate.prepush",
            crate::failure::Code::Refused,
            result.to_string(),
        ));
    }
    Ok(())
}

/// Whether the gate's verdict lets the push through: only an `ok` that is
/// the boolean true. `truthy` read any non-empty string, the text "false"
/// among them, and any non-zero number as a pass, so a verdict written in
/// the wrong type let a push through the release gate.
fn passed(result: &Value) -> bool {
    property(result, "ok").and_then(Value::as_bool) == Some(true)
}

/// The working tree a gate command acts on: `--repo` when given, the
/// directory the command runs in otherwise, as the flag documents.
fn repository(declared: &Option<PathBuf>) -> Result<PathBuf, Failure> {
    match declared {
        Some(repo) => Ok(repo.clone()),
        None => Ok(std::env::current_dir()?),
    }
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Whether the hook at `file` is one gate install wrote. A hook that cannot
/// be read is refused naming it: read as "not ours", uninstall refused to
/// touch it and install moved it aside, both for a cause nobody saw.
pub(crate) fn managed(
    file: &Path,
    legacy_command: &str,
    rust_command: &str,
) -> Result<bool, Failure> {
    let content = fs::read_to_string(file).map_err(|error| {
        Failure::config("gate.hook", format!("{} cannot be read: {error}", file.display()))
    })?;
    Ok(content.contains(MANAGED_MARKER)
        || (content.contains(legacy_command) && content.contains("--hook --app"))
        || (content.contains(rust_command)
            && content.contains("gate-prepush")
            && content.contains("--hook")))
}

pub fn install(harness: &Path, args: &InstallArgs) -> Answer {
    manifest::load(harness, &args.app_id)?;
    let repo = repository(&args.repo)?;
    let hooks = repo.join(".git").join("hooks");
    if !present(&hooks)? {
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
    if present(&backup)? && managed(&backup, &legacy_command, &rust_command)? {
        fs::remove_file(&backup)?;
    }
    if present(&target)?
        && !present(&backup)?
        && !managed(&target, &legacy_command, &rust_command)?
    {
        fs::rename(&target, &backup)?;
    }
    let script = format!(
        "#!/bin/sh\n{MANAGED_MARKER}\nHOOK_DIR=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\nif [ -f \"$HOOK_DIR/pre-push.before-probierz-gate\" ]; then\n  \"$HOOK_DIR/pre-push.before-probierz-gate\" \"$@\" || exit $?\nfi\nGATE_CI=\"--ci\"\nif [ \"${{PROBIERZ_GATE_NO_CI:-}}\" = \"1\" ]; then GATE_CI=\"\"; fi\nexec {} --harness {} gate prepush --hook --app {} $GATE_CI\n",
        shell_quote(&rust_command),
        shell_quote(&harness.to_string_lossy()),
        shell_quote(&args.app_id),
    );
    fs::create_dir_all(&hooks)?;
    fs::write(&target, script)?;
    // git runs the hook: its owner may change it, and everyone may read and
    // run it, as git installs its own sample hooks.
    let runnable = libc::S_IRWXU | libc::S_IRGRP | libc::S_IXGRP | libc::S_IROTH | libc::S_IXOTH;
    fs::set_permissions(&target, fs::Permissions::from_mode(u32::from(runnable)))?;
    print_json(&object([
        (
            "installed",
            Value::String(target.to_string_lossy().into_owned()),
        ),
        ("chained", Value::Bool(present(&backup)?)),
        ("appId", Value::String(args.app_id.clone())),
    ]))
}

/// The inverse of [`install`]: the managed pre-push hook leaves the working
/// tree and the hook it chained, kept as `pre-push.before-probierz-gate`, is
/// put back. A `pre-push` this command did not write is refused by name and
/// left alone; a tree with no managed hook is reported unchanged.
pub fn uninstall(harness: &Path, args: &InstallArgs) -> Answer {
    let repo = repository(&args.repo)?;
    let hooks = repo.join(".git").join("hooks");
    if !present(&hooks)? {
        return Err(Failure::config(
            "gate.uninstall",
            format!("not a git working tree: {}", repo.display()),
        ));
    }
    let target = hooks.join("pre-push");
    let backup = hooks.join("pre-push.before-probierz-gate");
    let rust_command = std::env::current_exe()?.to_string_lossy().into_owned();
    let legacy_command = harness
        .join("agent")
        .join("prepush-gate.mjs")
        .to_string_lossy()
        .into_owned();
    let removed = present(&target)?;
    if removed && !managed(&target, &legacy_command, &rust_command)? {
        return Err(Failure::config(
            "gate.uninstall",
            format!(
                "{} is not the hook gate install wrote; it was left as it is",
                target.display()
            ),
        ));
    }
    if removed {
        fs::remove_file(&target)?;
    }
    let restored = present(&backup)?;
    if restored {
        fs::rename(&backup, &target)?;
    }
    print_json(&object([
        ("removed", Value::Bool(removed)),
        ("restored", Value::Bool(restored)),
        ("hook", Value::String(target.to_string_lossy().into_owned())),
        ("appId", Value::String(args.app_id.clone())),
    ]))
}
