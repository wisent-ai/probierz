//! The one writer of a product's roadmap items in the catalog Stado serves.
//!
//! An item stays in the roadmap once written. What a benchmark case or an
//! incident asks of the product is planned while it is open, and recorded
//! delivered with the decision that ended it once it is met, so the history
//! of what was asked reads back from `stado product catalog`. An ended item is
//! never written again: a case or incident that comes back after delivery is
//! planned as an item of its own, under a title its caller makes unique, so
//! the delivered item keeps its decision. Removing an item is only for one
//! recorded in error, which nothing here does.

use std::process::Command;

use serde_json::{json, Value as Json};

use super::{record, stado};
use crate::failure::{Code, Failure};
use crate::stado::STADO_BIN;

/// The statuses this writer sets, in the words of Stado's `RoadmapStatus`.
pub(crate) const PLANNED: &str = "planned";
const DELIVERED: &str = "delivered";

/// Which roadmap statuses a decision has ended, as the catalog answer
/// declares them (`roadmap_statuses`: `{status, decided}` for every status
/// Stado knows). Probierz keeps no list of its own.
pub(crate) struct Statuses(Vec<(String, bool)>);

/// The roadmap statuses a `stado product catalog --json` answer declares; an
/// answer without them, or with an entry that does not state both fields, is
/// refused by name.
pub(crate) fn statuses(catalog: &Json) -> Result<Statuses, Failure> {
    let unread = |detail: String| Failure::config("benchmark.roadmap", detail);
    let Some(declared) = catalog["roadmap_statuses"].as_array() else {
        return Err(unread(format!(
            "stado product catalog --json declares no roadmap_statuses list ({}), so whether an item is still open cannot be read; a Stado that declares them is needed",
            catalog["roadmap_statuses"]
        )));
    };
    let mut read = Vec::new();
    for entry in declared {
        match (entry["status"].as_str(), entry["decided"].as_bool()) {
            (Some(status), Some(decided)) => read.push((status.to_string(), decided)),
            _ => {
                return Err(unread(format!(
                    "stado product catalog --json declares the roadmap status {entry} without both status and decided"
                )))
            }
        }
    }
    Ok(Statuses(read))
}

impl Statuses {
    /// Whether no decision has ended `item` yet; an item whose status the
    /// catalog does not declare is refused naming it.
    pub(crate) fn is_open(&self, item: &Json) -> Result<bool, String> {
        let status = item["status"].as_str();
        match self.0.iter().find(|(known, _)| Some(known.as_str()) == status) {
            Some((_, decided)) => Ok(!decided),
            None => Err(format!(
                "roadmap item {} has status {}, which the catalog's roadmap_statuses do not declare",
                item["title"], item["status"]
            )),
        }
    }
}

/// Product `id`'s roadmap items; a record whose roadmap is not a list is
/// refused, never read as empty.
fn items<'a>(id: &str, product: &'a Json) -> Result<&'a [Json], Failure> {
    match &product["roadmap"] {
        Json::Array(items) => Ok(items),
        other => Err(Failure::config(
            "benchmark.roadmap",
            format!("{id}'s catalog roadmap is {other}, not a list"),
        )),
    }
}

/// Every item of product `id` whose title starts with `prefix`, open or
/// ended.
pub(crate) fn titled(id: &str, product: &Json, prefix: &str) -> Result<Vec<Json>, Failure> {
    Ok(items(id, product)?
        .iter()
        .filter(|item| {
            item["title"]
                .as_str()
                .is_some_and(|title| title.starts_with(prefix))
        })
        .cloned()
        .collect())
}

/// The open items of `items`, as `statuses` reads them; an item whose status
/// is not declared is refused naming it.
pub(crate) fn open_of<'a>(
    statuses: &Statuses,
    items: &'a [Json],
) -> Result<Vec<&'a Json>, Failure> {
    let mut open = Vec::new();
    for item in items {
        if statuses
            .is_open(item)
            .map_err(|detail| Failure::config("benchmark.roadmap", detail))?
        {
            open.push(item);
        }
    }
    Ok(open)
}

/// `item` recorded delivered, ended by `decision`; its title, outcome and
/// source stay as they were.
pub(crate) fn delivered(item: &Json, decision: &str) -> Json {
    let mut ended = item.clone();
    ended["status"] = json!(DELIVERED);
    ended["decision"] = json!(decision);
    ended
}

/// Add or replace `written` (by title) in product `id`'s roadmap through
/// `stado product registry set`, then read the catalog back and refuse
/// unless it holds every item with the status and decision written.
pub(crate) fn write_roadmap(id: &str, written: &[Json]) -> Result<(), Failure> {
    if written.is_empty() {
        return Ok(());
    }
    let mut write = Command::new(STADO_BIN);
    write.args(["product", "registry", "set", id]);
    for item in written {
        write.arg("--add-roadmap").arg(item.to_string());
    }
    stado(&mut write)?;
    let product = record(id)?;
    let held = items(id, &product)?;
    let unkept: Vec<&Json> = written
        .iter()
        .filter(|item| {
            !held.iter().any(|stored| {
                stored["title"] == item["title"]
                    && stored["status"] == item["status"]
                    && stored["decision"] == item["decision"]
            })
        })
        .map(|item| &item["title"])
        .collect();
    if unkept.is_empty() {
        return Ok(());
    }
    Err(Failure::new(
        "benchmark.roadmap",
        Code::Unknown,
        format!(
            "the catalog read back after the write does not hold these items with the status written: {unkept:?}"
        ),
    ))
}
