use std::collections::{BTreeMap, HashMap, HashSet};
use std::thread;
use std::time::{Duration, Instant};

use regex::Regex;

use crate::{cua::Driver, specs};

use crate::specs::cua::common;

fn panel_is_rendered(tree: &str, title: &str) -> bool {
    tree.matches(&format!("AXStaticText = \"{title}\"")).count() >= 2
}

fn wait_for_tree<F>(
    driver: &Driver,
    pid: u32,
    window_id: u64,
    description: &str,
    predicate: F,
) -> Result<String, String>
where
    F: Fn(&str) -> bool,
{
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut tree = String::new();
    while Instant::now() < deadline {
        tree = driver.snapshot(pid, window_id)?.tree;
        if predicate(&tree) {
            return Ok(tree);
        }
        thread::sleep(Duration::from_millis(400));
    }
    Err(format!(
        "Timed out waiting for {description}; last tree (tail): {}",
        common::tail(&tree, 800)
    ))
}

pub fn run(context: &specs::Context) -> Result<(), String> {
    let executable = common::executable(context, "path to the Tama native application executable")?;
    let driver = common::driver(context)?;
    let environment = BTreeMap::from([("TAMA_TEST_IDENTITY".to_string(), "1".to_string())]);
    let app = driver.launch_process(&executable, &environment, &[])?;
    let result = (|| {
        driver.wait_for_text(
            app.pid,
            app.window_id,
            "AXOutline (Sidebar)",
            Duration::from_secs(30),
        )?;
        driver.select_sidebar_row(app.pid, app.window_id, 4)?;
        let path_pattern =
            Regex::new(r#"repo-githooks/[^\"]+/pre-[a-z-]+"#).expect("hook path regex");
        let repository = wait_for_tree(
            &driver,
            app.pid,
            app.window_id,
            "the Repository hooks panel and its installed hook rows",
            |tree| panel_is_rendered(tree, "Repository hooks") && path_pattern.is_match(tree),
        )?;
        let values = common::static_texts(&repository);
        let installed: Vec<String> = values
            .iter()
            .filter(|value| {
                Regex::new(r#"^repo-githooks/[^/]+/pre-[a-z-]+$"#)
                    .unwrap()
                    .is_match(value)
            })
            .cloned()
            .collect();
        if installed.is_empty() {
            return Err("Repository hooks should render at least one installed hook".to_string());
        }
        if installed.iter().collect::<HashSet<_>>().len() != installed.len() {
            return Err("Each installed repository hook should render once".to_string());
        }
        let event_pattern = Regex::new(r#"^pre-[a-z-]+$"#).expect("event regex");
        let mut expected = HashMap::new();
        for path in &installed {
            *expected
                .entry(path.rsplit('/').next().unwrap_or_default().to_string())
                .or_insert(0usize) += 1;
        }
        let mut rendered = HashMap::new();
        for event in values.iter().filter(|value| event_pattern.is_match(value)) {
            *rendered.entry(event.clone()).or_insert(0usize) += 1;
        }
        if rendered != expected {
            return Err(
                "Every installed repository hook should render its event status".to_string(),
            );
        }

        driver.snapshot(app.pid, app.window_id)?;
        driver.press_key(app.pid, Some(app.window_id), "up")?;
        wait_for_tree(
            &driver,
            app.pid,
            app.window_id,
            "the intermediate Snapshot validation panel",
            |tree| {
                panel_is_rendered(tree, "Snapshot validation")
                    && !tree.contains("AXStaticText = \"repo-githooks/")
            },
        )?;
        driver.snapshot(app.pid, app.window_id)?;
        driver.press_key(app.pid, Some(app.window_id), "up")?;
        let justifications = wait_for_tree(
            &driver,
            app.pid,
            app.window_id,
            "the Justifications panel content",
            |tree| {
                panel_is_rendered(tree, "Justifications")
                    && !tree.contains("AXStaticText = \"repo-githooks/")
                    && (tree.contains("AXRadioButton")
                        || tree.contains("AXStaticText = \"Registry unavailable\"")
                        || tree.contains("AXStaticText = \"No justification hooks\""))
            },
        )?;
        if !(justifications.contains("AXRadioButton")
            || justifications.contains("AXStaticText = \"Registry unavailable\"")
            || justifications.contains("AXStaticText = \"No justification hooks\""))
        {
            return Err(
                "Justifications should render its registry controls or an explicit content state"
                    .to_string(),
            );
        }
        Ok(())
    })();
    driver.quit_app(app.pid);
    result
}
