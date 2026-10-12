//! A product's first-use journey, run the way a new user meets it.
//!
//! `probierz run first-use/<app_id>` reads the product manifest's `firstUse:`
//! section (`declare`), gives the run a clean home of its own (`play`),
//! installs the product from its public install instructions, walks the
//! screens of its first-use definition in the order a user meets them
//! (`walk`), runs the command each screen shows with the user's own data, and
//! reads the first-success fact where the product records the journey.
//!
//! The record lands at `test-results/<app_id>/first-use/<started>/record.json`
//! beside every step's output, and is printed. The run fails unless the
//! product recorded the first-success fact with the value the definition
//! expects: an install or a shown command that did not succeed, or a fact
//! the product never recorded, is a failed first use, named in the record.

mod declare;
mod play;
mod walk;

use std::path::Path;

use serde_json::json;

use crate::failure::{now_iso, print_json, Answer, Code, Failure};

/// The target prefix that names a product's first-use journey to `probierz run`.
pub(crate) const TARGET_PREFIX: &str = "first-use/";

pub(crate) fn run(harness: &Path, app_id: &str) -> Answer {
    let manifest = crate::manifest::load(harness, app_id)?;
    let declared = declare::declared(&manifest)?;
    let started = now_iso();
    let root = harness
        .join("test-results")
        .join(&manifest.app_id)
        .join("first-use")
        .join(started.replace([':', '.'], "-"));
    let sandbox = play::Sandbox::create(root.clone(), &declared)?;
    let mut record = play::play(&declared, &sandbox)?;
    record["appId"] = json!(manifest.app_id);
    record["journey"] = declared.describe();
    let file = root.join("record.json");
    std::fs::write(&file, serde_json::to_vec_pretty(&record)?)?;
    print_json(&record)?;
    if record["passed"] == true {
        return Ok(());
    }
    Err(Failure::new(
        "first-use.run",
        Code::Refused,
        format!(
            "{}'s first use did not reach its first-success fact {} (installed: {}, walked: {}); the record names the step: {}",
            manifest.app_id,
            record["firstSuccess"]["fact"],
            record["installed"],
            record["walked"],
            file.display()
        ),
    ))
}
