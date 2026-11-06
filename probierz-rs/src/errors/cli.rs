//! `probierz refusals <list|show|check>`: the refusals Probierz declares
//! (`crate::errors`), and the check that every cause names a real test.

use std::path::Path;

use serde_json::Value;

use super::{problems, OPERATIONS};
use crate::cli::reporting::RefusalsCommand;
use crate::failure::{print_json, Answer, Failure, FAILURE_EXIT};

pub fn run(harness: &Path, command: RefusalsCommand) -> Answer {
    match command {
        RefusalsCommand::List { markdown } => list(markdown),
        RefusalsCommand::Show { id, markdown } => show(&id, markdown),
        RefusalsCommand::Check { root } => check(root.as_deref().unwrap_or(harness)),
    }
}

/// `text`, as the catalogue renders it, parsed for the answer printer.
fn parsed(text: &str) -> Result<Value, Failure> {
    serde_json::from_str(text).map_err(|error| {
        Failure::config(
            "refusals",
            format!("the declared refusals did not render as JSON: {error}"),
        )
    })
}

fn list(markdown: bool) -> Answer {
    if markdown {
        let sections: Vec<String> = OPERATIONS
            .iter()
            .map(|operation| operation.markdown(FAILURE_EXIT))
            .collect();
        crate::capture::emit(&format!("{}\n", sections.join("\n")));
        return Ok(());
    }
    let operations = OPERATIONS
        .iter()
        .map(|operation| parsed(&operation.to_json(FAILURE_EXIT)))
        .collect::<Result<Vec<Value>, Failure>>()?;
    print_json(&serde_json::json!({ "operations": operations }))
}

fn show(id: &str, markdown: bool) -> Answer {
    let Some(operation) = OPERATIONS.iter().find(|operation| operation.id == id) else {
        let declared: Vec<&str> = OPERATIONS.iter().map(|operation| operation.id).collect();
        return Err(Failure::invalid(
            "refusals.show",
            format!(
                "no operation {id} declares its refusals; the operations that do: {}",
                declared.join(", ")
            ),
        ));
    };
    if markdown {
        crate::capture::emit(&format!("{}\n", operation.markdown(FAILURE_EXIT)));
        return Ok(());
    }
    print_json(&parsed(&operation.to_json(FAILURE_EXIT))?)
}

fn check(root: &Path) -> Answer {
    let found = problems(root);
    if !found.is_empty() {
        return Err(Failure::invalid(
            "refusals.check",
            format!(
                "the declared refusals do not meet the error standard under {}: {}",
                root.display(),
                found.join("; ")
            ),
        ));
    }
    let causes: usize = OPERATIONS.iter().map(|operation| operation.causes.len()).sum();
    print_json(&serde_json::json!({
        "operations": OPERATIONS.len(),
        "causes": causes,
        "problems": [],
    }))
}
