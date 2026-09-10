use crate::stado::*;
pub fn dispatch(harness: &Path, command: StadoCommand) -> Answer {
    match command {
        StadoCommand::Run(args) => dispatch_run(harness, args),
        StadoCommand::Collect(args) => dispatch_collect(harness, args),
        StadoCommand::Resume(args) => dispatch_resume(harness, args),
        StadoCommand::Cancel(args) => dispatch_cancel(harness, args),
        StadoCommand::Author(args) => dispatch_author(harness, args),
        StadoCommand::Seo(args) => dispatch_seo(harness, args),
        StadoCommand::AuthorReceipt(args) => write_author_receipt(harness, args),
        StadoCommand::BykAuthWorker => byk_auth_worker(),
    }
}

pub(crate) fn dispatch_run(harness: &Path, args: RunArgs) -> Answer {
    let target = args
        .target
        .as_deref()
        .ok_or_else(|| Failure::config("stado.run", "stado run needs a target (e.g. tui)"))?;
    let app_id = args
        .app
        .as_deref()
        .ok_or_else(|| Failure::config("stado.run", "stado run needs --app <appId>"))?;
    let environment = parse_environment(&args.env)?;
    let provision = select_run_provision(&args, app_id, target)?;
    if args.script.is_some() && !matches!(provision, Some(Provision::NodeSource { .. })) {
        return Err(Failure::config(
            "stado.run",
            "--script requires --node-source (custom app jobs run from app sources)",
        ));
    }
    let result = submit_remote_run(
        harness,
        target,
        app_id,
        args.spec.as_deref(),
        &args.host,
        provision,
        args.app_repo.as_deref(),
        !args.no_watch,
        if args.script.is_some() {
            "script"
        } else {
            "run"
        },
        args.record,
        &environment,
    )?;
    finish_remote(result)
}

pub(crate) fn dispatch_collect(harness: &Path, args: CollectArgs) -> Answer {
    let app_id = args
        .app
        .ok_or_else(|| Failure::config("stado.collect", "stado collect needs --app <appId>"))?;
    let result = collect_remote_run(harness, args.job_id.as_deref(), &app_id, &args.host)?;
    let must_finish = result
        .get("collected")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || matches!(
            result.get("state").and_then(Value::as_str),
            Some("failed" | "cancelled" | "evidence-unavailable")
        );
    print_json(&result)?;
    if must_finish {
        remote_result_answer(&result)
    } else {
        Ok(())
    }
}

pub(crate) fn dispatch_resume(harness: &Path, args: ResumeArgs) -> Answer {
    let result = resume_remote_run(harness, args.job_id.as_deref(), &args.host)?;
    finish_remote(result)
}

pub(crate) fn dispatch_cancel(harness: &Path, args: CancelArgs) -> Answer {
    let host = args
        .host
        .ok_or_else(|| Failure::config("stado.cancel", "stado cancel needs --host <host>"))?;
    let reason = args
        .reason
        .ok_or_else(|| Failure::config("stado.cancel", "stado cancel needs --reason <reason>"))?;
    let result = cancel_remote_run(harness, args.job_id.as_deref(), &host, &reason)?;
    print_json(&result)?;
    if result.get("cancellationSucceeded").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        remote_result_answer(&result)
    }
}

pub(crate) fn dispatch_author(harness: &Path, args: AuthorArgs) -> Answer {
    let app_id = args.app_id.as_deref().ok_or_else(|| {
        Failure::config(
            "stado.author",
            "stado author needs an app ID and a journey name",
        )
    })?;
    let journey = args.journey.as_deref().ok_or_else(|| {
        Failure::config(
            "stado.author",
            "stado author needs an app ID and a journey name",
        )
    })?;
    let target = args
        .target
        .as_deref()
        .ok_or_else(|| Failure::config("stado.author", "stado author needs --target <t>"))?;
    let desc = args.desc.as_deref().ok_or_else(|| {
        Failure::config("stado.author", "stado author needs --desc <journey goal>")
    })?;
    let provision = select_author_provision(&args, app_id, target)?;
    let result = submit_remote_author(
        harness,
        app_id,
        journey,
        target,
        desc,
        args.area.as_deref().unwrap_or(journey),
        &args.host,
        provision,
        args.app_repo.as_deref(),
        !args.no_watch,
    )?;
    finish_remote(result)
}

pub(crate) fn dispatch_seo(harness: &Path, args: SeoArgs) -> Answer {
    let app_id = args
        .app_id
        .clone()
        .ok_or_else(|| Failure::config("stado.seo", "stado seo needs an app ID"))?;
    let result = submit_remote_seo(harness, &app_id, args)?;
    finish_remote(result)
}

pub(crate) fn finish_remote(result: Value) -> Answer {
    print_json(&result)?;
    remote_result_answer(&result)
}

pub(crate) fn remote_result_answer(result: &Value) -> Answer {
    let state = result
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let queued = state == "queued"
        && result.get("submitted").and_then(Value::as_bool) == Some(true)
        && result.get("failure").map(Value::is_null).unwrap_or(true);
    if state == "completed" || queued {
        return Ok(());
    }
    let failure = result
        .get("cancellationFailure")
        .filter(|value| !value.is_null())
        .or_else(|| result.get("failure"));
    let point = failure
        .and_then(|value| value.get("failurePoint"))
        .and_then(Value::as_str)
        .unwrap_or("stado.remote");
    let message = failure
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("Remote run ended as \"{state}\"."));
    let retryable = failure
        .and_then(|value| value.get("retryable"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if retryable {
        Err(Failure::unavailable(point, message))
    } else {
        Err(Failure::config(point, message))
    }
}

pub(crate) fn parse_environment(values: &[String]) -> Result<Vec<(String, String)>, Failure> {
    let mut answer = Vec::with_capacity(values.len());
    for assignment in values {
        let Some((name, value)) = assignment.split_once('=') else {
            return Err(Failure::config(
                "stado.run",
                "--env needs NAME=VALUE with a valid environment variable name",
            ));
        };
        if !valid_environment_name(name) || value.contains('\0') {
            return Err(Failure::config(
                "stado.run",
                "--env needs NAME=VALUE with a valid environment variable name",
            ));
        }
        answer.push((name.to_string(), value.to_string()));
    }
    Ok(answer)
}

pub(crate) fn valid_environment_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

