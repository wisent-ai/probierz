use std::collections::HashSet;
use std::time::Duration;

use regex::Regex;

use crate::specs;

use super::console as console;

fn writing_controls(view: &console::View) -> Vec<String> {
    let writing = Regex::new(r"(?i)^(Reclaim|Clear|Apply|Restart|Stop|End|Kill|Terminate|Ensure|Install|Converge|Delete|Remove)\b").unwrap();
    let read_only: HashSet<&str> = [
        "Refresh",
        "Retry",
        "Show them",
        "Clear filters",
        "All units",
        "Serving replaced code",
        "Unowned processes",
        "Posture",
        "Queue",
        "Hosts",
        "Services",
        "Disk",
        "Registry",
        "Releases",
        "Deployments",
    ]
    .into_iter()
    .collect();
    console::buttons(view)
        .into_iter()
        .filter_map(|button| {
            let label = Regex::new(r",\s*[\d,]+$")
                .unwrap()
                .replace(&button.label, "")
                .to_string();
            (!read_only.contains(label.as_str()) && writing.is_match(&label))
                .then_some(button.label)
        })
        .collect()
}

fn assert_read_only(view: &console::View, where_: &str) -> Result<(), String> {
    let writes = writing_controls(view);
    if writes.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "the Services screen offers a mutating control {where_}: {}",
            writes.join(" | ")
        ))
    }
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
            "Services",
            |tree| {
                Regex::new(r"AX\w*Button \(All units, [1-9]")
                    .unwrap()
                    .is_match(tree)
            },
            "/AX\\w*Button \\(All units, [1-9]/",
            &[
                "No registry hosts to ask",
                "No host reported its units",
                "No declared units",
            ],
            "Refresh",
            Duration::from_secs(180),
        )?;
        let (_, row) = console::select_row(
            &driver,
            app.pid,
            app.window_id,
            |tree| tree.contains("PROCESS MATCHES PROGRAM ON DISK"),
            "/PROCESS MATCHES PROGRAM ON DISK/",
            Duration::from_secs(60),
            3,
            0,
        )?;
        let loaded = console::capture(
            context,
            &driver,
            app.pid,
            app.window_id,
            "stado-services-screen",
            "loaded",
        )?;
        for needle in [
            "No registry hosts to ask",
            "No host reported its units",
            "No declared units",
            "Reading declared units on",
            "No row selected",
        ] {
            if loaded.tree.contains(needle) {
                return Err(format!("the Services screen did not load declared units from the hosts: the screen shows {needle:?}"));
            }
        }
        let declared_version = console::assert_field(
            &loaded,
            "Declared version",
            None::<fn(&str) -> bool>,
            "the field",
        )?;
        let installed_version = console::assert_field(
            &loaded,
            "Installed version",
            None::<fn(&str) -> bool>,
            "the field",
        )?;
        if declared_version.is_empty() || installed_version.is_empty() {
            return Err(format!(
                "{} renders no declared/installed version pair",
                row.label
            ));
        }
        let declared_program = console::assert_field(
            &loaded,
            "Declared program",
            None::<fn(&str) -> bool>,
            "the field",
        )?;
        let running_binary = console::assert_field(
            &loaded,
            "Running binary",
            None::<fn(&str) -> bool>,
            "the field",
        )?;
        let process_match = console::assert_field(
            &loaded,
            "Process matches program on disk",
            Some(|value: &str| {
                value == "Yes" || value.starts_with("No") || value == "Not reported by this host"
            }),
            "/^(Yes|No|Not reported by this host)/",
        )?;
        if declared_program.is_empty() {
            return Err("the unit renders no declared program".to_string());
        }
        if running_binary == "Not reported" && process_match != "Not reported by this host" {
            return Err(
                "a host that named no running binary must not be rendered as a match".to_string(),
            );
        }
        if process_match.starts_with("No")
            && !loaded
                .tree
                .contains("The process is not executing the program on disk")
        {
            return Err(format!(
                "a replaced binary must be called that; tree: {}",
                crate::specs::cua::common::tail(&loaded.tree, 2000)
            ));
        }
        console::assert_field(&loaded, "Unit state", None::<fn(&str) -> bool>, "the field")?;
        console::assert_field(&loaded, "Verdict", None::<fn(&str) -> bool>, "the field")?;
        assert_read_only(&loaded, "beside a declared unit")?;

        let facet = console::button(
            &console::read_window(&driver, app.pid, app.window_id)?,
            "Unowned processes",
        )?;
        let unowned = Regex::new(r",\s*(\d+)$")
            .unwrap()
            .captures(&facet.label)
            .and_then(|capture| capture[1].parse::<usize>().ok())
            .unwrap_or(0);
        console::attempt(&driver, app.pid, app.window_id, &facet.label);
        if unowned > 0 {
            let (_, process) = console::select_row(
                &driver,
                app.pid,
                app.window_id,
                |tree| tree.contains("Nothing supervises this process"),
                "/Nothing supervises this process/",
                Duration::from_secs(60),
                3,
                0,
            )?;
            let reported = console::capture(
                context,
                &driver,
                app.pid,
                app.window_id,
                "stado-services-screen",
                "unowned-process",
            )?;
            let explanation = "No declared unit owns it, so no release updates it, nothing restarts it if it dies, and nothing stops it. Two processes in this state ran for four days before anybody looked. Ending it is a decision for whoever knows what it is doing, and this console does not make it.";
            if !reported.tree.contains(explanation) {
                return Err(format!(
                    "the unowned process {} is not reported in the screen's own words; tree: {}",
                    process.label,
                    crate::specs::cua::common::tail(&reported.tree, 2500)
                ));
            }
            console::assert_field(
                &reported,
                "PID",
                Some(|value: &str| Regex::new(r"^[\d,]+$").unwrap().is_match(value)),
                "/^[\\d,]+$/",
            )?;
            console::assert_field(&reported, "Command", None::<fn(&str) -> bool>, "the field")?;
            console::assert_field(
                &reported,
                "Product guess",
                None::<fn(&str) -> bool>,
                "the field",
            )?;
            assert_read_only(&reported, "beside an unowned process")?;
            if console::find_button(&reported, "End process").is_some()
                || console::find_button(&reported, "Stop").is_some()
            {
                return Err(
                    "the screen offers to end a process it says it does not end".to_string()
                );
            }
        } else {
            let empty = console::capture(
                context,
                &driver,
                app.pid,
                app.window_id,
                "stado-services-screen",
                "unowned-process",
            )?;
            if !(empty
                .tree
                .contains("Every product process belongs to a unit")
                || empty.tree.contains("Unowned processes are unknown"))
            {
                return Err(format!(
                    "the unowned facet reports neither processes nor their absence; tree: {}",
                    crate::specs::cua::common::tail(&empty.tree, 2000)
                ));
            }
            assert_read_only(&empty, "on the unowned facet")?;
        }
        Ok(())
    })();
    let _ = console::dump_windows(context, &driver, app.pid, "stado-services-screen");
    driver.quit_app(app.pid);
    result
}
