//! What the matrix reads before and around its cells: its own arguments,
//! the artifact key, and the record and evidence level of one cell.

use crate::run::*;

/// The matrix's own arguments: `--plan`, `--release <id>`, and the
/// `NAME=VALUE` pairs that reach every cell's environment.
pub(super) struct Arguments {
    pub(super) plan_only: bool,
    pub(super) release: Option<String>,
    pub(super) env: BTreeMap<String, String>,
}

pub(super) fn arguments(args: &[String]) -> Result<Arguments, Failure> {
    let mut parsed = Arguments {
        plan_only: false,
        release: None,
        env: BTreeMap::new(),
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--plan" => parsed.plan_only = true,
            "--release" => match args.next() {
                Some(release) => parsed.release = Some(release.clone()),
                None => return Err(fail("cli.matrix", "--release needs a release id")),
            },
            other => {
                let Some((name, value)) = other.split_once('=') else {
                    return Err(fail(
                        "cli.matrix",
                        format!("unexpected matrix argument: {other}"),
                    ));
                };
                parsed.env.insert(name.into(), value.into());
            }
        }
    }
    Ok(parsed)
}

/// The artifact key file: the matrix argument, else the environment. A
/// variable that is set but not text is refused naming it.
pub(super) fn artifact_key(env: &BTreeMap<String, String>) -> Result<Option<String>, Failure> {
    if let Some(key) = env.get("PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE") {
        return Ok(Some(key.clone()));
    }
    match std::env::var("PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE") {
        Ok(key) => Ok(Some(key)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(fail(
            "run.matrix",
            "PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE is not text",
        )),
    }
}

/// A cell's environment as the plan declares it; a value that is not text
/// is refused naming it.
pub(super) fn declared_env(cell: &Value) -> Result<BTreeMap<String, String>, Failure> {
    let mut env = BTreeMap::new();
    if let Some(declared) = cell.get("env").and_then(Value::as_object) {
        for (name, value) in declared {
            let Some(text) = value.as_str() else {
                return Err(fail(
                    "run.matrix",
                    format!("matrix cell env {name} is not text: {value}"),
                ));
            };
            env.insert(name.clone(), text.to_string());
        }
    }
    Ok(env)
}

/// Adds `fields` to a cell's public record; a plan cell that is not an
/// object is refused naming it.
pub(super) fn extend(public: &mut Value, fields: Value) -> Result<(), Failure> {
    let Some(record) = public.as_object_mut() else {
        return Err(fail(
            "run.matrix",
            format!("a matrix cell is not an object: {public}"),
        ));
    };
    if let Value::Object(fields) = fields {
        record.extend(fields);
    }
    Ok(())
}

/// The evidence level a completed cell run reaches, by the one rule every
/// reader applies: it passed when it states `passed` true.
pub(super) fn cell_level(completed: &Value) -> &'static str {
    crate::gate::level_of(
        crate::gate::is_true(completed.get("passed")),
        completed.get("conditions"),
        completed.get("evidence"),
    )
}
