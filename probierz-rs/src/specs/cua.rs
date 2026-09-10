//! Journeys that run against native desktop applications over the accessibility tree.

use super::Spec;

pub(crate) mod common;


/// Every journey registered for this surface. Titles retain the old spec
/// basenames because they are report identity, not implementation filenames.
pub fn specs() -> Vec<Spec> {
    vec![
        Spec {
            surface: "desktop:cua",
            title: "brama-desktop-critical-operations",
            run: brama::desktop_critical_operations::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "brama-desktop-subscription-pool-screen",
            run: brama::desktop_subscription_pool_screen::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "jeden-desktop-task-contract",
            run: jeden::desktop_task_contract::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "skarbiec-desktop-capabilities-screen",
            run: skarbiec::desktop_capabilities_screen::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "apple-challenge-desktop",
            run: stado::apple_challenge_desktop::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "host-dynamic-capacity",
            run: stado::host_dynamic_capacity::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "service-convergence",
            run: stado::service_convergence::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "stado-hosts-screen",
            run: stado::screens::hosts_screen::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "stado-releases-screen",
            run: stado::screens::releases_screen::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "stado-services-screen",
            run: stado::screens::services_screen::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "tama-auth-gate",
            run: tama::auth_gate::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "tama-session-control",
            run: tama::session_control::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "tama-system-policy",
            run: tama::system_policy::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "tama-violations-panel",
            run: tama::violations_panel::run,
        },
    ]
}
mod brama;
mod jeden;
mod skarbiec;
mod stado;
mod tama;
