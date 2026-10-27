//! A product repository's own test suites, run in its checkout and kept.
//!
//! A checkout's release manifest declares, per platform, the quality gates a
//! release passes; a gate of kind `test` is one of the product's own suites
//! (a Cargo test target, an XCTest target, a `node --test` journey). The
//! target `suites/<checkout>` names them to `probierz run`, as
//! `first-use/<app>` names a first-use journey: `probierz check
//! suites/<checkout>` lists the declared suites and the newest kept run, and
//! `probierz run suites/<checkout> [<gate>…]` runs them where they live and
//! keeps, under `test-results/.suites/<product>/`, one run directory per run:
//! the checkout's revision and dirty paths, every gate's argv, exit status,
//! duration, both streams, and the reports the gate declares, copied as the
//! suite left them. Nothing in a kept run is rewritten.

mod identity;
mod manifest;
mod record;
mod run;

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::failure::{print_json, Answer, Code, Failure};

pub(crate) const MANIFEST_FILE: &str = ".wisent-release.json";
pub(crate) const TEST_KIND: &str = "test";
pub(crate) const RUN_SCHEMA: &str = "ai.wisent.probierz.suite-run.v1";

/// The target prefix that names a checkout's own suites to `probierz run`
/// and `probierz check`.
pub(crate) const TARGET_PREFIX: &str = "suites/";

/// Where the runs of one product are kept; a product name that is not one
/// plain path component is refused rather than joined.
fn runs_root(harness: &Path, product: &str) -> Result<PathBuf, Failure> {
    let mut components = Path::new(product).components();
    match (components.next(), components.next()) {
        (Some(std::path::Component::Normal(name)), None) if name == product => {
            Ok(harness.join("test-results").join(".suites").join(product))
        }
        _ => Err(Failure::invalid(
            "suites.record",
            format!(
                "the manifest names product {product:?}, which is not one plain directory name"
            ),
        )),
    }
}

/// `probierz check suites/<checkout>`: every suite the checkout declares for
/// this platform, and the newest kept run of its product when one exists.
pub(crate) fn check(harness: &Path, repo: &Path) -> Answer {
    let gates = manifest::test_gates(repo)?;
    let product = manifest::product_name(repo)?;
    let root = runs_root(harness, &product)?;
    // A runs folder that cannot be examined is not one with no runs.
    let kept = root.try_exists().map_err(|error| {
        Failure::unavailable(
            "suites.record",
            format!("the kept runs {} cannot be examined: {error}", root.display()),
        )
    })?;
    let last = if kept {
        Some(record::newest(&root)?)
    } else {
        None
    };
    print_json(&json!({
        "product": product,
        "declared": manifest::listing(repo, &gates),
        "runs": root,
        "lastRun": last,
    }))
}

/// `probierz run suites/<checkout> [<gate>…]`: the named gates, or every
/// declared one, run in the checkout and kept; a failed gate refuses the run
/// after its record is printed.
pub(crate) fn run(harness: &Path, repo: &Path, gates: &[String]) -> Answer {
    let declared = manifest::test_gates(repo)?;
    let selected = manifest::select(repo, declared, gates)?;
    let root = runs_root(harness, &selected.product)?;
    let recorded = run::run(repo, &selected, &root)?;
    print_json(&recorded.document)?;
    if recorded.failed {
        return Err(Failure::new(
            "suites.run",
            Code::Refused,
            format!(
                "{} of {} suites failed; the run is kept in {}",
                recorded.failed_count,
                recorded.gate_count,
                recorded.directory.display()
            ),
        ));
    }
    Ok(())
}
