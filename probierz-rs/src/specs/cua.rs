//! Journeys that run against native desktop applications over the accessibility tree.

use super::Spec;

mod common;
mod stado_console;

mod brama_desktop_critical_operations;
mod brama_desktop_subscription_pool_screen;
mod jeden_desktop_task_contract;
mod skarbiec_desktop_capabilities_screen;
mod stado_apple_challenge_desktop;
mod stado_host_dynamic_capacity;
mod stado_hosts_screen;
mod stado_releases_screen;
mod stado_service_convergence;
mod stado_services_screen;
mod tama_auth_gate;
mod tama_session_control;
mod tama_system_policy;
mod tama_violations_panel;

/// Every journey registered for this surface. Titles retain the old spec
/// basenames because they are report identity, not implementation filenames.
pub fn specs() -> Vec<Spec> {
    vec![
        Spec {
            surface: "desktop:cua",
            title: "brama-desktop-critical-operations",
            run: brama_desktop_critical_operations::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "brama-desktop-subscription-pool-screen",
            run: brama_desktop_subscription_pool_screen::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "jeden-desktop-task-contract",
            run: jeden_desktop_task_contract::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "skarbiec-desktop-capabilities-screen",
            run: skarbiec_desktop_capabilities_screen::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "apple-challenge-desktop",
            run: stado_apple_challenge_desktop::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "host-dynamic-capacity",
            run: stado_host_dynamic_capacity::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "service-convergence",
            run: stado_service_convergence::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "stado-hosts-screen",
            run: stado_hosts_screen::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "stado-releases-screen",
            run: stado_releases_screen::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "stado-services-screen",
            run: stado_services_screen::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "tama-auth-gate",
            run: tama_auth_gate::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "tama-session-control",
            run: tama_session_control::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "tama-system-policy",
            run: tama_system_policy::run,
        },
        Spec {
            surface: "desktop:cua",
            title: "tama-violations-panel",
            run: tama_violations_panel::run,
        },
    ]
}
