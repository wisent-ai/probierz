use std::collections::BTreeMap;
use std::time::Duration;

use regex::Regex;

use crate::{cua, specs};

use crate::specs::cua::common;

pub fn run(context: &specs::Context) -> Result<(), String> {
    let executable = common::executable(context, "path to the Tama native application executable")?;
    let driver = common::driver(context)?;
    let environment = BTreeMap::from([("TAMA_TEST_IDENTITY".to_string(), "1".to_string())]);
    let app = driver.launch_process(&executable, &environment, &[])?;
    let result = (|| {
        driver.wait_for_text(
            app.pid,
            app.window_id,
            "AXStaticText = \"Violations\"",
            Duration::from_secs(30),
        )?;
        driver.select_sidebar_row(app.pid, app.window_id, 5)?;
        let scan = driver.wait_for_text(
            app.pid,
            app.window_id,
            "AXButton (Scan)",
            Duration::from_secs(30),
        )?;
        common::require_contains(&scan.tree, "AXStaticText = \"No scan yet\"", || {
            format!(
                "the Violations panel does not begin in No scan yet: {}",
                scan.tree
            )
        })?;
        let index = cua::element_index_of(&scan.tree, "AXButton (Scan)")?;
        let element = scan
            .element_by_index(index)
            .ok_or_else(|| "no indexed element matching \"AXButton (Scan)\" in tree".to_string())?;
        driver.click_element(app.pid, app.window_id, &scan, element)?;
        let completed = driver.wait_for_text(
            app.pid,
            app.window_id,
            "AXStaticText = \"Total violations\"",
            Duration::from_secs(120),
        )?;
        common::require_absent(&completed.tree, "AXStaticText = \"Scan failed\"", || {
            format!("the Tama scan failed: {}", completed.tree)
        })?;

        let labels = [
            "Files scanned",
            "Skipped files",
            "Scan errors",
            "Total violations",
        ];
        let count = Regex::new(r#"AXStaticText = "[0-9][0-9,]*""#).expect("count regex");
        for (position, label) in labels.iter().enumerate() {
            let marker = format!("AXStaticText = \"{label}\"");
            let start = completed
                .tree
                .find(&marker)
                .ok_or_else(|| format!("{label} should be shown"))?;
            let end = labels
                .get(position + 1)
                .and_then(|next| {
                    completed.tree[start..]
                        .find(&format!("AXStaticText = \"{next}\""))
                        .map(|offset| start + offset)
                })
                .unwrap_or(completed.tree.len());
            if end <= start {
                return Err(format!("{label} should precede the next summary count"));
            }
            if !count.is_match(&completed.tree[start..end]) {
                return Err(format!("{label} should show a numeric count"));
            }
        }
        Ok(())
    })();
    driver.quit_app(app.pid);
    result
}
