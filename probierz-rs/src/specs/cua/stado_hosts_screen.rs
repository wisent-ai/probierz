use std::time::Duration;

use regex::Regex;

use crate::specs;

use super::stado_console as console;

pub fn run(context: &specs::Context) -> Result<(), String> {
    super::common::executable(context, "path to the Stado native application executable")?;
    let driver = super::common::driver(context)?;
    let app = console::launch_console(context, &driver)?;
    let result = (|| {
        console::open_screen(
            &driver,
            app.pid,
            app.window_id,
            "Hosts",
            |tree| {
                Regex::new(r"AX\w*Button \(All hosts")
                    .unwrap()
                    .is_match(tree)
            },
            "/AX\\w*Button \\(All hosts/",
            &["No host inventory", "No registered hosts"],
            "Refresh",
            Duration::from_secs(180),
        )?;
        let (_, row) = console::select_row(
            &driver,
            app.pid,
            app.window_id,
            |tree| tree.contains("CLEANUP POLICY MODE"),
            "/CLEANUP POLICY MODE/",
            Duration::from_secs(60),
            3,
            0,
        )?;
        let loaded = console::capture(
            context,
            &driver,
            app.pid,
            app.window_id,
            "stado-hosts-screen",
            "loaded",
        )?;
        for needle in [
            "No host inventory",
            "No registered hosts",
            "No hosts in this filter",
            "Reading host capacity reports",
            "No host selected",
        ] {
            if loaded.tree.contains(needle) {
                return Err(format!(
                    "the Hosts screen did not load real fleet state: the screen shows {needle:?}"
                ));
            }
        }
        let claiming = console::assert_field(
            &loaded,
            "Claiming work",
            Some(|value: &str| matches!(value, "Yes" | "No")),
            "/^(Yes|No)$/",
        )?;
        let blockers =
            console::assert_field(&loaded, "Blockers", None::<fn(&str) -> bool>, "the field")?;
        if blockers == "Reading…" {
            return Err(format!(
                "{} renders a spinner where its blockers belong",
                row.label
            ));
        }
        if claiming == "No" && !loaded.tree.contains("This host is claiming no work") {
            return Err(format!(
                "a host that claims nothing must say so; tree: {}",
                super::common::tail(&loaded.tree, 2000)
            ));
        }
        let free_pattern =
            Regex::new(r"^([\d.,]+ GB free|Not reported)").expect("free space regex");
        let free = console::assert_field(
            &loaded,
            "Free space",
            Some(|value: &str| free_pattern.is_match(value)),
            "/^([\\d.,]+ GB free|Not reported)/",
        )?;
        console::assert_field(
            &loaded,
            "Cleanup policy mode",
            None::<fn(&str) -> bool>,
            "the field",
        )?;
        if !Regex::new(r"[\d.,]+ GB").unwrap().is_match(&loaded.tree) {
            return Err("the screen renders no disk figure for any host".to_string());
        }

        let (sheet_window, sheet, _) = console::activate(
            &driver,
            app.pid,
            app.window_id,
            "Reclaim disk…",
            "\"Reclaim disk on \"",
            |tree| tree.contains("Reclaim disk on "),
            Duration::from_secs(120),
        )?;
        if !sheet.tree.contains("Why this host needs the space") {
            return Err(format!(
                "the reclamation sheet does not ask why; tree: {}",
                super::common::tail(&sheet.tree, 2000)
            ));
        }
        if ![
            "Type a reason to enable the apply.",
            "The apply stays unavailable until the dry run above has answered for",
        ]
        .iter()
        .any(|text| sheet.tree.contains(text))
        {
            return Err(format!(
                "the sheet does not state why the apply is unavailable; tree: {}",
                super::common::tail(&sheet.tree, 2500)
            ));
        }
        if !Regex::new(
            r#"stado host reclaim .*--apply --reason "why this host needs the space" --json"#,
        )
        .unwrap()
        .is_match(&sheet.tree)
        {
            return Err(
                "the sheet does not show the apply command with an unfilled reason".to_string(),
            );
        }
        let control = console::assert_refused_control(&sheet, "Reclaim now")?;
        let refused = console::capture(
            context,
            &driver,
            app.pid,
            sheet_window,
            "stado-hosts-screen",
            "refused-without-reason",
        )?;
        if !refused.tree.contains("Why this host needs the space") {
            return Err(format!(
                "the sheet stopped asking why; tree: {}",
                super::common::tail(&refused.tree, 2000)
            ));
        }
        if console::assert_refused_control(&refused, "Reclaim now")? != control {
            return Err("the apply became reachable without a reason being typed".to_string());
        }
        for needle in [
            "What reclamation freed",
            "Reclaiming disk on ",
            "The pass ran and reported no stages",
            "A reason is required",
        ] {
            if refused.tree.contains(needle) {
                return Err(format!("the screen applied a reclamation without a typed reason: the screen shows {needle:?}"));
            }
        }
        if !refused
            .tree
            .contains("--apply --reason \"why this host needs the space\" --json")
        {
            return Err("the refused sheet no longer shows the unfilled apply command".to_string());
        }
        console::attempt(&driver, app.pid, sheet_window, "Cancel");
        let after = console::wait_for_screen(
            &driver,
            app.pid,
            app.window_id,
            |tree| tree.contains("CLEANUP POLICY MODE"),
            "/CLEANUP POLICY MODE/",
            Duration::from_secs(120),
        )?;
        for needle in ["What reclamation freed", "Reclaim disk on "] {
            if after.tree.contains(needle) {
                return Err(format!(
                    "the reclamation sheet outlived a cancel: the screen shows {needle:?}"
                ));
            }
        }
        let after_free = console::assert_field(
            &after,
            "Free space",
            Some(|value: &str| free_pattern.is_match(value)),
            "/^([\\d.,]+ GB free|Not reported)/",
        )?;
        if after_free != free {
            return Err(
                "this host's free space changed while the journey refused to reclaim anything"
                    .to_string(),
            );
        }
        let after_claiming = console::assert_field(
            &after,
            "Claiming work",
            Some(|value: &str| matches!(value, "Yes" | "No")),
            "/^(Yes|No)$/",
        )?;
        if after_claiming != claiming {
            return Err(
                "this host's claiming gate changed while the journey refused to reclaim anything"
                    .to_string(),
            );
        }
        Ok(())
    })();
    let _ = console::dump_windows(context, &driver, app.pid, "stado-hosts-screen");
    driver.quit_app(app.pid);
    result
}
