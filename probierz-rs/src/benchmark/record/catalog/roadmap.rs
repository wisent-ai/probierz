//! `probierz benchmark roadmap <app> --suite <id>`: the newest run's losses
//! as the product's catalog roadmap.
//!
//! Every case our contender lost is one planned item, replaced with the
//! newest run's words each time. A case still open in the roadmap that the
//! newest run no longer loses was won or tied by it: it stays in the roadmap
//! as delivered, with that run's id as its decision, so the product's history
//! of what the benchmark asked of it reads back. A case lost again after its
//! item was delivered is planned as a new item, titled with the run that lost
//! it, and the delivered item keeps its decision.

use std::path::Path;

use serde_json::{json, Value as Json};

use super::super::commands::standing_of;
use super::ledger::{self, PLANNED};
use super::{product_id, record};
use crate::failure::{print_json, Answer, Failure};
use crate::manifest;

/// What follows a case's title in the item of a case lost again after its
/// earlier item was delivered: the run that lost it.
const LOST_AGAIN: &str = ", lost again in run ";

fn title(suite: &str, case: &str) -> String {
    format!("Win benchmark case {suite}/{case}")
}

/// The case an item of the suite whose titles start with `prefix` is about.
fn case_of<'a>(item: &'a Json, prefix: &str) -> Option<&'a str> {
    let rest = item["title"].as_str()?.strip_prefix(prefix)?;
    Some(match rest.split_once(LOST_AGAIN) {
        Some((case, _)) => case,
        None => rest,
    })
}

pub(crate) fn roadmap(harness: &Path, app_id: &str, suite_id: &str) -> Answer {
    print_json(&roadmap_of(harness, app_id, suite_id)?)
}

/// Bring the product's catalog roadmap in line with the newest run of one
/// suite and answer what was planned and what was delivered.
pub(crate) fn roadmap_of(harness: &Path, app_id: &str, suite_id: &str) -> Result<Json, Failure> {
    let manifest = manifest::load(harness, app_id)?;
    let product = product_id(&manifest);
    let standing = standing_of(harness, app_id, suite_id)?;
    // A roadmap item names the run, our contender and the case it came from;
    // one read as '' would be written into the product's catalog.
    let stated = |value: &Json, field: &str| {
        value[field].as_str().map(str::to_string).ok_or_else(|| {
            Failure::config(
                "benchmark.roadmap",
                format!("the {suite_id} standing of {app_id} states no {field}"),
            )
        })
    };
    let run_id = stated(&standing, "runId")?;
    let ours = stated(&standing, "ours")?;
    let losses = standing["losses"].as_array().ok_or_else(|| {
        Failure::config(
            "benchmark.roadmap",
            format!("the {suite_id} standing of {app_id} lists no losses"),
        )
    })?;
    let prefix = title(suite_id, "");
    let items = ledger::titled(&product, &record(&product)?, &prefix)?;
    let open: Vec<&Json> = items.iter().filter(|item| ledger::is_open(item)).collect();
    let mut planned = Vec::new();
    let mut lost = Vec::new();
    for loss in losses {
        let case = stated(loss, "case")?;
        let no_winners = || {
            Failure::config(
                "benchmark.roadmap",
                format!("case {case} of the {suite_id} standing of {app_id} names no winners"),
            )
        };
        let winners = loss["winners"]
            .as_array()
            .ok_or_else(no_winners)?
            .iter()
            .map(|winner| winner.as_str().ok_or_else(no_winners))
            .collect::<Result<Vec<&str>, Failure>>()?;
        let about = |item: &&Json| case_of(item, &prefix) == Some(case.as_str());
        // The case's open item is refreshed under its own title; a case whose
        // every item was delivered is planned under a title of its own.
        let item_title = match open.iter().copied().find(|item| about(item)) {
            Some(item) => item["title"].clone(),
            None if items.iter().any(|item| about(&item)) => {
                json!(format!("{}{LOST_AGAIN}{run_id}", title(suite_id, &case)))
            }
            None => json!(title(suite_id, &case)),
        };
        planned.push(json!({
            "title": item_title,
            "status": PLANNED,
            "outcome": format!(
                "probierz benchmark standing {app_id} --suite {suite_id} shows {ours} winning or tying case {case}, which {} won",
                winners.join(", ")
            ),
            "source": format!("probierz benchmark run {run_id} of {app_id}"),
        }));
        lost.push(case);
    }
    let delivered: Vec<Json> = open
        .iter()
        .filter(|item| case_of(item, &prefix).is_some_and(|case| !lost.iter().any(|known| known == case)))
        .map(|item| ledger::delivered(item, &run_id))
        .collect();
    let written: Vec<Json> = planned.iter().chain(&delivered).cloned().collect();
    ledger::write_roadmap(&product, &written)?;
    let delivered_titles: Vec<&Json> = delivered.iter().map(|item| &item["title"]).collect();
    Ok(json!({
        "appId": app_id,
        "product": product,
        "runId": run_id,
        "added": planned,
        "delivered": delivered_titles,
    }))
}
