use crate::run::*;
pub(crate) struct Target {
    pub(crate) pkg: &'static str,
    pub(crate) script: &'static str,
    pub(crate) tool: &'static str,
}

pub(crate) fn target(name: &str) -> Option<Target> {
    Some(match name {
        "web" => Target {
            pkg: "packages/web",
            script: "test:web",
            tool: "playwright",
        },
        "electron" => Target {
            pkg: "packages/electron",
            script: "test:electron",
            tool: "playwright",
        },
        "mobile:ios" => Target {
            pkg: "packages/mobile",
            script: "test:mobile:ios",
            tool: "wdio",
        },
        "mobile:ios:byk-auth" => Target {
            pkg: "packages/mobile",
            script: "test:mobile:ios:byk-auth",
            tool: "wdio",
        },
        "mobile:android" => Target {
            pkg: "packages/mobile",
            script: "test:mobile:android",
            tool: "wdio",
        },
        "desktop:mac" => Target {
            pkg: "packages/desktop-native",
            script: "test:desktop:mac",
            tool: "wdio",
        },
        "desktop:win" => Target {
            pkg: "packages/desktop-native",
            script: "test:desktop:win",
            tool: "wdio",
        },
        "desktop:cua" => Target {
            pkg: "packages/desktop-cua",
            script: "probierz run desktop:cua",
            tool: "cua-driver",
        },
        "tui" => Target {
            pkg: "packages/tui",
            script: "probierz run tui",
            tool: "probierz",
        },
        _ => return None,
    })
}

pub(crate) fn target_list() -> Vec<&'static str> {
    vec![
        "web",
        "electron",
        "mobile:ios",
        "mobile:ios:byk-auth",
        "mobile:android",
        "desktop:mac",
        "desktop:win",
        "desktop:cua",
        "tui",
    ]
}

pub(crate) fn accepted_preflight_targets() -> &'static str {
    "web|electron|mobile:ios|mobile:ios:byk-auth|mobile:android|desktop:mac|desktop:cua|desktop:win|tui"
}

/// The flags the six execution commands accept, printed by each of their
/// `--help` screens.
///
/// They share one parser, so they share one description. Help that omits a
/// flag the parser accepts is the same defect as a documented command the
/// binary does not have: the declaration stops matching the world.
/// The shared flag text, as a macro so that a command needing more can
/// `concat!` its own block onto it without a string-building dependency.
#[macro_export]
macro_rules! run_flags_help {
    () => {
        "\
Accepted arguments (parsed by the shared execution parser):
  NAME=VALUE            Environment variable given to the suite; repeatable
  --app <ID>            Application manifest whose surface and secrets apply
  --spec <FILE>         One spec file instead of the target's whole suite
  --tool <NAME>         Report shape to expect: playwright, wdio, or probierz
  --record              Keep video, traces, and screenshots for every journey
  --force               Run even when the resource this target locks is held
  --no-analyze          Skip report analysis and print the raw run
  --no-repair           Do not offer an authored repair for a failed run
  --frames <N>          Frames per second to extract from a recording
  --timeout <MS>        Give the suite this long before it is killed
  --resource-wait <MS>  Wait this long for a held resource before refusing
  --files <PATH>...     Changed files that select what runs (affected, ci)
  --host <SELECTOR>     mobile:ios:byk-auth only: the fleet host its suite
                        is placed on, from `probierz hosts`; default
                        stado:mini
  --local               mobile:ios:byk-auth only: run its suite on this
                        machine instead of the dedicated host
  --seed-resend         mobile:ios:byk-auth only: seed the login mailbox's
                        resend source and stop, running no journey"
    };
}

pub const RUN_FLAGS_HELP: &str = run_flags_help!();

/// The two flags only `matrix` accepts, appended to its own help.
/// The two flags only `matrix` accepts.
#[macro_export]
macro_rules! matrix_flags_help {
    () => {
        "\
Matrix-only arguments:
  --plan                Print the matrix this app and profile resolve to,
                        running nothing
  --release <ID>        The release the matrix runs against; required when the
                        profile is `release` and the matrix is executed"
    };
}

#[derive(Default)]
pub(crate) struct RunArgs {
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) record: bool,
    pub(crate) analyze: bool,
    pub(crate) force: bool,
    pub(crate) no_repair: bool,
    pub(crate) app_id: Option<String>,
    pub(crate) spec: Option<String>,
    pub(crate) frames: f64,
    pub(crate) timeout_ms: u64,
    pub(crate) resource_wait_ms: Option<u64>,
    pub(crate) tool: Option<String>,
    /// Run the byk-auth worker on this machine instead of the dedicated host.
    pub(crate) local: bool,
    /// The fleet host the remote byk-auth suite is placed on.
    pub(crate) host: Option<String>,
    /// Seed the login mailbox's resend source and stop, without a journey.
    pub(crate) seed_resend: bool,
}

pub(crate) fn parse_non_negative(flag: &str, value: &str) -> Result<f64, Failure> {
    let parsed = value.parse::<f64>().map_err(|_| {
        fail(
            "cli.arguments",
            format!("{flag} needs a non-negative number"),
        )
    })?;
    if !parsed.is_finite() || parsed < 0.0 {
        return Err(fail(
            "cli.arguments",
            format!("{flag} needs a non-negative number"),
        ));
    }
    Ok(parsed)
}

pub(crate) fn value_after(args: &[String], index: usize, flag: &str) -> Result<String, Failure> {
    let value = args.get(index + 1).filter(|value| !value.starts_with("--"));
    value
        .cloned()
        .ok_or_else(|| fail("cli.arguments", format!("{flag} needs a value")))
}

pub(crate) fn parse_run_args(args: &[String], allow_positionals: bool) -> Result<RunArgs, Failure> {
    let mut opts = RunArgs {
        analyze: true,
        ..RunArgs::default()
    };
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--record" => opts.record = true,
            "--force" => opts.force = true,
            "--no-repair" => opts.no_repair = true,
            "--no-analyze" => opts.analyze = false,
            "--frames" => {
                let value = value_after(args, index, arg)?;
                opts.frames = parse_non_negative(arg, &value)?;
                index += 1;
            }
            "--timeout" => {
                let value = value_after(args, index, arg)?;
                opts.timeout_ms = parse_non_negative(arg, &value)? as u64;
                index += 1;
            }
            "--resource-wait" => {
                let value = value_after(args, index, arg)?;
                opts.resource_wait_ms = Some(parse_non_negative(arg, &value)? as u64);
                index += 1;
            }
            "--spec" => {
                opts.spec = Some(value_after(args, index, arg)?);
                index += 1;
            }
            "--app" => {
                opts.app_id = Some(value_after(args, index, arg)?);
                index += 1;
            }
            "--tool" => {
                opts.tool = Some(value_after(args, index, arg)?);
                index += 1;
            }
            "--local" => opts.local = true,
            "--host" => {
                opts.host = Some(value_after(args, index, arg)?);
                index += 1;
            }
            "--seed-resend" => opts.seed_resend = true,
            "--files" => {}
            _ if arg.starts_with("--") => {
                return Err(fail("cli.arguments", format!("unknown option: {arg}")))
            }
            _ if arg.contains('=') => {
                let (name, value) = arg.split_once('=').unwrap_or((arg, ""));
                opts.env.insert(name.to_string(), value.to_string());
            }
            _ if !allow_positionals => {
                return Err(fail("cli.arguments", format!("unexpected argument: {arg}")))
            }
            _ => {}
        }
        index += 1;
    }
    Ok(opts)
}

pub(crate) fn files_after_flag(args: &[String]) -> Option<Vec<String>> {
    let start = args.iter().position(|arg| arg == "--files")? + 1;
    let valued = [
        "--frames",
        "--timeout",
        "--resource-wait",
        "--spec",
        "--app",
        "--tool",
    ];
    let mut files = Vec::new();
    let mut index = start;
    while index < args.len() {
        if valued.contains(&args[index].as_str()) {
            index += 2;
        } else {
            if !args[index].starts_with("--") && !args[index].contains('=') {
                files.push(args[index].clone());
            }
            index += 1;
        }
    }
    Some(files)
}

