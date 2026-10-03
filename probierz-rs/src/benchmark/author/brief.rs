//! What the model is told when it drafts a contender driver or a suite, and
//! how its answer is read back. The briefs name the contract and the rules
//! once, for every product; only the catalog record and the suite differ.

use serde::Deserialize;
use serde_json::{json, Value as Json};

use crate::benchmark::inputs::suite::Loaded;
use crate::benchmark::{RESULT_SCHEMA, SUITE_SCHEMA, TASK_SCHEMA};

/// What a contender driver is drafted for: our product or a named rival.
pub(crate) struct Subject {
    pub ours: bool,
    pub record: Json,
}

/// One drafted driver, as the model returns it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Driver {
    pub file: String,
    pub env: Vec<String>,
    pub source: String,
}

/// The draft of the round before, and what its verification found.
pub(crate) struct Previous {
    pub source: String,
    pub failures: Vec<String>,
}

const CONTRACT: &str = "\
The contract is the driver's only interface:
- Probierz starts the driver as an executable file, with an empty environment except the variables you list in \"env\". Start the file with a shebang.
- stdin carries one JSON document: {\"schema\": \"TASK\", \"suite\": {\"id\", \"version\", \"hash\"}, \"case\": {\"id\", \"instruction\", \"input\"}, \"repetition\": n}. Placeholders in the input are already filled.
- stdout carries exactly one JSON document and nothing else: {\"schema\": \"RESULT\", \"status\": \"completed\" or \"failed\", \"output\": the JSON object the instruction asks for, \"steps\": integer, \"tokens\": integer, \"costUsd\": number, \"error\": string}. Only those keys; steps, tokens, costUsd and error may be left out. Every log line goes to stderr.
- A task the product could not do is answered with status \"failed\" and the reason in \"error\"; the driver exits 0 whenever it wrote a result.";

const RULES: &str = "\
Rules:
- Drive the product the way its own documentation drives it, through its published SDK, CLI or API. Never reimplement what it does.
- Give the product the case instruction and the case input, unchanged, and nothing about the expected answer: every contender gets the same words.
- Every setting comes from a variable listed in \"env\": credentials, endpoints, step limits, and the directory the product's packages are installed in (<ID>_MODULES or <ID>_SITE_PACKAGES, imported from there, never vendored). List PATH and HOME too. A missing variable is a failed result naming it; there are no defaults for credentials or endpoints.
- Every language-model call the product makes goes through Brama's OpenAI-compatible endpoint, from BRAMA_BASE_URL, BRAMA_API_KEY and BRAMA_MODEL, never a provider key.
- A browser the product launches itself runs headless from BROWSER_EXECUTABLE_PATH.
- No sleeping or polling loops in your code: wait on the product's own completion call.
- One self-contained file named contender.<extension>, in the language of the product's own SDK.";

fn suite_cases(loaded: &Loaded) -> String {
    let cases: Vec<Json> = loaded
        .suite
        .cases
        .iter()
        .map(|case| json!({"id": case.id, "instruction": case.instruction, "input": case.input}))
        .collect();
    serde_json::to_string_pretty(&json!({
        "id": loaded.suite.id,
        "version": loaded.suite.version,
        "variables": loaded.suite.variables,
        "cases": cases,
    }))
    .expect("a suite serializes")
}

pub(crate) fn contender(
    subject: &Subject,
    loaded: &Loaded,
    previous: Option<&Previous>,
    round: u32,
    rounds: u32,
) -> String {
    let who = if subject.ours {
        "our own product (the contender the benchmark compares every rival against)"
    } else {
        "a rival product the catalog names"
    };
    let language = if subject.ours {
        "Our product's driver is never Python."
    } else {
        "Python is allowed for a rival whose only SDK is Python."
    };
    let mut brief = format!(
        "Write the benchmark contender driver for {who}, round {round} of {rounds}.\n\n\
         The product, as the Wisent product catalog records it:\n{}\n\n\
         {}\n\n{RULES}\n- {language}\n\n\
         The suite it answers (expected answers are hidden from you and from the driver):\n{}\n\n\
         Call the tool once with one JSON object: {{\"file\": \"contender.<extension>\", \"env\": [variable names], \"source\": \"<the complete file>\"}}.",
        serde_json::to_string_pretty(&subject.record).expect("a catalog record serializes"),
        CONTRACT.replace("TASK", TASK_SCHEMA).replace("RESULT", RESULT_SCHEMA),
        suite_cases(loaded),
    );
    if let Some(previous) = previous {
        brief.push_str(&format!(
            "\n\nThe previous driver did not satisfy the contract or failed with an error. Fix it.\n--- PREVIOUS DRIVER ---\n{}\n--- WHAT ITS RECORDED RUN FOUND ---\n{}",
            previous.source,
            previous.failures.join("\n")
        ));
    }
    brief
}

/// Read a drafted driver, or say what is wrong with it so the next round
/// can fix it.
pub(crate) fn driver(content: &str, ours: bool) -> Result<Driver, String> {
    let driver: Driver = serde_json::from_str(content).map_err(|error| {
        format!("the draft is not a {{file, env, source}} JSON object: {error}")
    })?;
    let named = driver
        .file
        .strip_prefix("contender.")
        .is_some_and(|extension| {
            !extension.is_empty() && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
        });
    if !named {
        return Err(format!("file {} is not contender.<extension>", driver.file));
    }
    if ours && driver.file.ends_with(".py") {
        return Err("our product's driver is never Python".to_string());
    }
    if !driver.source.starts_with("#!") {
        return Err(
            "the source does not start with a shebang, so it cannot be executed".to_string(),
        );
    }
    if let Some(name) = driver.env.iter().find(|name| {
        name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    }) {
        return Err(format!("env names {name:?}, which is not a variable name"));
    }
    Ok(driver)
}

pub(crate) fn suite(
    product: &Json,
    suite_id: &str,
    cases: usize,
    previous: Option<&Previous>,
) -> String {
    let record = json!({
        "id": product["id"],
        "name": product["name"],
        "description": product["description"],
        "docs_origin": product["docs_origin"],
        "rivals": product["rivals"],
    });
    let mut brief = format!(
        "Write benchmark suite {suite_id} for the product below and every rival it names: {cases} cases on which each of them can be asked to do the same task, judged only by assertions on a JSON answer.\n\n\
         The product, as the Wisent product catalog records it:\n{}\n\n\
         The suite is one JSON document: {{\"schema\": \"{SUITE_SCHEMA}\", \"id\": \"{suite_id}\", \"version\": \"1.0.0\", \"repetitions\": 1, \"variables\": {{\"PLACEHOLDER\": \"ENV_VARIABLE\"}}, \"cases\": [{{\"id\": \"kebab-case-id\", \"instruction\": \"...\", \"input\": {{...}}, \"assertions\": [{{\"pointer\": \"/field\", \"equals\": value}}, {{\"pointer\": \"/field\", \"exists\": true}}, {{\"pointer\": \"/field\", \"includes\": \"text\"}}]}}]}}. Each assertion sets exactly one of equals, exists, includes.\n\n\
         Rules:\n\
         - The cases test the promise the product's documentation makes, on tasks every rival also claims to do through its public interface.\n\
         - Every case is deterministic: its input fixes the right answer, so a correct contender always passes and a wrong one always fails.\n\
         - The instruction says exactly which JSON object to answer, with which keys.\n\
         - What differs between hosts (an origin, an endpoint) is a ${{PLACEHOLDER}} in the input, and variables maps it to the environment variable that holds it. No credential appears in a case.\n\
         - The expected values appear only in assertions; nothing in an instruction or input gives them away.\n\n\
         Call the tool once with the complete suite document.",
        serde_json::to_string_pretty(&record).expect("a catalog record serializes"),
    );
    if let Some(previous) = previous {
        brief.push_str(&format!(
            "\n\nThe previous draft was refused. Fix it.\n--- PREVIOUS DRAFT ---\n{}\n--- WHY IT WAS REFUSED ---\n{}",
            previous.source,
            previous.failures.join("\n")
        ));
    }
    brief
}
