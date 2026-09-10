use std::time::Duration;

use regex::Regex;

use crate::specs;

use super::console as console;

fn clearable_digests(view: &console::View) -> usize {
    console::buttons(view)
        .into_iter()
        .filter(|button| button.label == "Clear…")
        .count()
}

pub fn run(context: &specs::Context) -> Result<(), String> {
    crate::specs::cua::common::executable(context, "path to the Stado native application executable")?;
    let driver = crate::specs::cua::common::driver(context)?;
    let app = console::launch_console(context, &driver)?;
    let result = (|| {
        console::open_screen(
            &driver,
            app.pid,
            app.window_id,
            "Releases",
            |tree| tree.contains("DESIRED VERSION"),
            "\"DESIRED VERSION\"",
            &[
                "No rollout could be listed",
                "Nothing was read",
                "No rollout is declared",
            ],
            "Re-diagnose",
            Duration::from_secs(240),
        )?;
        let loaded = console::capture(
            context,
            &driver,
            app.pid,
            app.window_id,
            "stado-releases-screen",
            "loaded",
        )?;
        for needle in [
            "No rollout could be listed",
            "Re-diagnosis failed",
            "Nothing was read",
            "No rollout is declared",
            "Diagnosing every declared rollout",
        ] {
            if loaded.tree.contains(needle) {
                return Err(format!("the Releases screen did not load real rollout state: the screen shows {needle:?}"));
            }
        }
        let desired = console::assert_field(
            &loaded,
            "Desired version",
            None::<fn(&str) -> bool>,
            "the field",
        )?;
        let observed = console::assert_field(
            &loaded,
            "Observed version",
            None::<fn(&str) -> bool>,
            "the field",
        )?;
        let phase = console::assert_field(&loaded, "Phase", None::<fn(&str) -> bool>, "the field")?;
        if phase == "—" {
            return Err(format!("the rollout renders no phase: {phase:?}"));
        }
        let verdict = Regex::new(r"\b(settled|rolling|blocked|unreported)\b")
            .unwrap()
            .captures(&loaded.tree)
            .map(|capture| capture[1].to_string())
            .ok_or_else(|| {
                format!(
                    "the rollout carries no verdict word from the CLI; tree: {}",
                    crate::specs::cua::common::tail(&loaded.tree, 2000)
                )
            })?;
        let blockers =
            console::assert_field(&loaded, "Blockers", None::<fn(&str) -> bool>, "the field")?;
        if verdict == "blocked" && blockers == "None. Nothing is holding this rollout." {
            return Err("a blocked rollout must name what is holding it".to_string());
        }
        if verdict == "settled" && observed != desired {
            return Err(
                "a settled rollout renders the observed version as the desired one".to_string(),
            );
        }
        console::assert_field(
            &loaded,
            "Disk pressure",
            Some(|value: &str| matches!(value, "Resolved" | "Unresolved")),
            "/^(Resolved|Unresolved)/",
        )?;
        console::assert_field(
            &loaded,
            "Free space",
            Some(|value: &str| value.contains("GB") || value.contains('—')),
            "/GB|—/",
        )?;

        let mut quarantined = console::poll(
            &driver,
            app.pid,
            app.window_id,
            |tree| tree.contains("Clear…"),
            Duration::from_secs(90),
        )?;
        if quarantined.is_none() {
            let rows =
                console::row_buttons(&console::read_window(&driver, app.pid, app.window_id)?, 3)
                    .len();
            for skip in 1..rows {
                if quarantined.is_some() {
                    break;
                }
                console::select_row(
                    &driver,
                    app.pid,
                    app.window_id,
                    |tree| {
                        tree.contains("Clear…")
                            || tree.contains("Nothing is quarantined for")
                            || tree.contains("Clearing is unavailable")
                    },
                    "/Clear…|Nothing is quarantined for|Clearing is unavailable/",
                    Duration::from_secs(90),
                    3,
                    skip,
                )?;
                quarantined = console::poll(
                    &driver,
                    app.pid,
                    app.window_id,
                    |tree| tree.contains("Clear…"),
                    Duration::from_secs(90),
                )?;
            }
        }
        let quarantined = quarantined.ok_or_else(|| {
            let tree = console::read_window(&driver, app.pid, app.window_id).map(|view| view.tree).unwrap_or_default();
            format!("no rollout on this fleet holds a quarantined digest, so the screen's only write could not be reached; tree: {}", crate::specs::cua::common::tail(&tree, 2500))
        })?;
        let held = clearable_digests(&quarantined);
        if held == 0 {
            return Err("the quarantine pane offers no digest to clear".to_string());
        }
        let (dialog_window, dialog, _) = console::activate(
            &driver,
            app.pid,
            app.window_id,
            "Clear…",
            "\"Clear digest\"",
            |tree| tree.contains("Clear digest"),
            Duration::from_secs(30),
        )?;
        if !dialog
            .tree
            .contains("REQUIRED, RECORDED IN THE AUDIT TRAIL")
        {
            return Err(format!(
                "the clearance dialog does not state that a reason is required; tree: {}",
                crate::specs::cua::common::tail(&dialog.tree, 2000)
            ));
        }
        let refusal = "Without a reason this command does not run. It is what an audit reads months from now, when nobody remembers why the digest was given another chance.";
        if !dialog.tree.contains(refusal) {
            return Err(format!(
                "the dialog does not refuse an empty reason in its own words; tree: {}",
                crate::specs::cua::common::tail(&dialog.tree, 2000)
            ));
        }
        if !Regex::new(r#"stado release quarantine clear .*--reason "<reason>" --json"#)
            .unwrap()
            .is_match(&dialog.tree)
        {
            return Err(
                "the dialog does not show the exact command with an unfilled reason".to_string(),
            );
        }
        let control = console::assert_refused_control(&dialog, "Clear digest")?;
        let refused = console::capture(
            context,
            &driver,
            app.pid,
            dialog_window,
            "stado-releases-screen",
            "refused-without-reason",
        )?;
        if !refused
            .tree
            .contains("REQUIRED, RECORDED IN THE AUDIT TRAIL")
        {
            return Err(format!(
                "the dialog stopped asking for a reason; tree: {}",
                crate::specs::cua::common::tail(&refused.tree, 2000)
            ));
        }
        if console::assert_refused_control(&refused, "Clear digest")? != control {
            return Err("the clearance became reachable without a reason being typed".to_string());
        }
        if !refused.tree.contains("--reason \"<reason>\" --json") {
            return Err("the refused dialog no longer shows the unfilled command".to_string());
        }
        let behind = console::read_window(&driver, app.pid, app.window_id)?;
        let clearing = Regex::new(r"Clearing sha256:|Clearing [0-9a-f]{12}").unwrap();
        for (description, present) in [
            ("\"LAST CLEARANCE\"", behind.tree.contains("LAST CLEARANCE")),
            (
                "/Clearing sha256:|Clearing [0-9a-f]{12}/",
                clearing.is_match(&behind.tree),
            ),
            (
                "\"Previous state backed up on the host at\"",
                behind
                    .tree
                    .contains("Previous state backed up on the host at"),
            ),
        ] {
            if present {
                return Err(format!("the screen cleared a digest without a typed reason: the screen shows {description}"));
            }
        }
        console::attempt(&driver, app.pid, dialog_window, "Leave it quarantined");
        let after = console::wait_for_screen(
            &driver,
            app.pid,
            app.window_id,
            |tree| tree.contains("Clear…"),
            "\"Clear…\"",
            Duration::from_secs(30),
        )?;
        let after_count = clearable_digests(&after);
        if after_count != held {
            return Err(format!("the host's quarantine map changed: {held} clearable digests before, {after_count} after"));
        }
        Ok(())
    })();
    let _ = console::dump_windows(context, &driver, app.pid, "stado-releases-screen");
    driver.quit_app(app.pid);
    result
}
