//! The jeden journeys this surface runs, and their registry.

use crate::specs::Spec;

pub(crate) mod cli;
pub(crate) mod functional;
pub(crate) mod model_routing;
pub(crate) mod onboarding_first_use;
pub(crate) mod sandbox;
pub(crate) mod scan;
pub(crate) mod screens;
pub(crate) mod settings_screen;
pub(crate) mod task_contract_lifecycle;
pub(crate) mod views;

type Journey = fn(&crate::specs::Context) -> Result<(), String>;

/// Every jeden journey: title and function, all on the tui surface.
const JOURNEYS: [(&str, Journey); 44] = [
    ("jeden-command-scan", scan::command_scan),
    ("jeden-exit", scan::exit),
    ("jeden-side-by-side", functional::sidebyside::side_by_side),
    ("jeden-perf-local", cli::perf::local),
    ("jeden-perf-network", cli::perf::network),
    ("jeden-confirm-guards", functional::keys::confirm_guards),
    ("jeden-pane-crossing", functional::keys::pane_crossing),
    (
        "jeden-selection-applies",
        functional::keys::selection_applies,
    ),
    ("jeden-screen-geometry", screens::semantics::geometry),
    ("jeden-screen-replacement", screens::semantics::replacement),
    (
        "jeden-screen-transcript-budget",
        screens::semantics::transcript_budget,
    ),
    ("jeden-screen-two-pane", screens::semantics::two_pane),
    ("jeden-model-loading-state", screens::model_loading),
    ("jeden-turn-busy-state", screens::turn_busy),
    ("jeden-usage-loading-state", screens::usage_loading),
    ("jeden-agent-discovery", views::discovery::agents),
    ("jeden-branch-roundtrip", functional::state::branch),
    ("jeden-checkpoint-roundtrip", functional::state::checkpoint),
    ("jeden-cli-basics", cli::basics),
    ("jeden-cli-collab-share", cli::collab_share),
    ("jeden-cli-doctor", cli::network::doctor),
    ("jeden-cli-gallery", cli::gallery),
    ("jeden-cli-model-catalog", cli::network::model_catalog),
    ("jeden-cli-run", cli::network::run),
    ("jeden-cli-settings-export", cli::settings_export),
    ("jeden-cli-token", cli::network::token),
    ("jeden-cli-usage", cli::network::usage),
    ("jeden-collab-relay", functional::files::collab),
    ("jeden-extension-discovery", views::discovery::extensions),
    ("jeden-identity", views::replays::identity),
    ("jeden-marketplace-source", functional::files::marketplace),
    ("jeden-mode-roundtrip", functional::state::plan_mode),
    (
        "jeden-model-picker-after-login",
        views::replays::model_picker_after_login,
    ),
    ("jeden-model-routing", model_routing::run),
    ("jeden-omfg-persists", functional::files::omfg),
    ("jeden-onboarding-first-use", onboarding_first_use::run),
    ("jeden-rename-roundtrip", functional::state::rename),
    ("jeden-settings-screen", settings_screen::run),
    (
        "jeden-settings-write-through",
        functional::files::settings_write_through,
    ),
    ("jeden-setup-checklist", views::discovery::setup_checklist),
    (
        "jeden-task-contract-lifecycle",
        task_contract_lifecycle::run,
    ),
    ("jeden-todo-roundtrip", functional::state::todo),
    ("jeden-token-redacted", functional::files::token_redacted),
    ("jeden-view-content", views::run),
];

/// The jeden journeys as registry entries.
pub(crate) fn specs() -> Vec<Spec> {
    JOURNEYS
        .iter()
        .map(|&(title, run)| Spec {
            surface: "tui",
            title,
            run,
        })
        .collect()
}
