//! The journeys that run against terminal applications, over a real PTY.

use super::Spec;

pub(crate) mod common;

mod game_asset_creator_cli;
mod ssh_auth_router_onboarding_first_use;

/// Every journey registered for this surface.
pub fn specs() -> Vec<Spec> {
    vec![
        Spec {
            surface: "tui",
            title: "adam-agent-toolkit-onboarding-first-use",
            run: adam::agent_toolkit_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "adam-services-onboarding-first-use",
            run: adam::services_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "brama-onboarding-first-use",
            run: brama::onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "brama-subscription-pool",
            run: brama::subscription_pool::run,
        },
        Spec {
            surface: "tui",
            title: "deep-analytics-onboarding-first-use",
            run: deep::analytics_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "echo-docs-capabilities",
            run: echo::docs_capabilities::run,
        },
        Spec {
            surface: "tui",
            title: "echo-read-every-capability",
            run: echo::read_every_capability::run,
        },
        Spec {
            surface: "tui",
            title: "echo-web-analytics-collectors",
            run: echo::web_analytics_collectors::run,
        },
        Spec {
            surface: "tui",
            title: "echo-web-capability-catalogue",
            run: echo::web_capability_catalogue::run,
        },
        Spec {
            surface: "tui",
            title: "game-asset-creator-cli",
            run: game_asset_creator_cli::run,
        },
        Spec {
            surface: "tui",
            title: "jeden-model-routing",
            run: jeden::model_routing::run,
        },
        Spec {
            surface: "tui",
            title: "jeden-onboarding-first-use",
            run: jeden::onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "jeden-settings-screen",
            run: jeden::settings_screen::run,
        },
        Spec {
            surface: "tui",
            title: "jeden-task-contract-lifecycle",
            run: jeden::task_contract_lifecycle::run,
        },
        Spec {
            surface: "tui",
            title: "las-onboarding-first-use",
            run: las::onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "most-onboarding-first-use",
            run: most::onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "oko-autonomy",
            run: oko::autonomy::run,
        },
        Spec {
            surface: "tui",
            title: "singularity-onboarding-first-use",
            run: singularity::onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "skrzynka-crate-builds",
            run: skrzynka::crate_builds::run,
        },
        Spec {
            surface: "tui",
            title: "skrzynka-mailbox-lifecycle",
            run: skrzynka::mailbox_lifecycle::run,
        },
        Spec {
            surface: "tui",
            title: "skrzynka-oauth-failure-diagnosis",
            run: skrzynka::oauth_failure_diagnosis::run,
        },
        Spec {
            surface: "tui",
            title: "ssh-auth-router-onboarding-first-use",
            run: ssh_auth_router_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "ugc-cli-onboarding-first-use",
            run: ugc::cli_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "weles-onboarding-first-use",
            run: weles::onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "wisent-benchmark-onboarding-first-use",
            run: wisent::benchmark_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "wisent-onboarding-first-use",
            run: wisent::onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "wisent-optimizer-onboarding-first-use",
            run: wisent::optimizer_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "skarbiec-delete-and-restore-secret",
            run: skarbiec::delete_and_restore_secret::run,
        },
        Spec {
            surface: "tui",
            title: "skarbiec-initialize-vault",
            run: skarbiec::initialize_vault::run,
        },
        Spec {
            surface: "tui",
            title: "skarbiec-manage-secrets",
            run: skarbiec::manage_secrets::run,
        },
        Spec {
            surface: "tui",
            title: "skarbiec-manage-service-routes",
            run: skarbiec::manage_service_routes::run,
        },
        Spec {
            surface: "tui",
            title: "skarbiec-manage-users-and-sharing",
            run: skarbiec::manage_users_and_sharing::run,
        },
        Spec {
            surface: "tui",
            title: "skarbiec-onboarding-first-use",
            run: skarbiec::onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "skarbiec-recover-vault-access",
            run: skarbiec::recover_vault_access::run,
        },
        Spec {
            surface: "tui",
            title: "skarbiec-use-json-output",
            run: skarbiec::use_json_output::run,
        },
        Spec {
            surface: "tui",
            title: "stado-host-gates",
            run: stado::host_gates::run,
        },
        Spec {
            surface: "tui",
            title: "stado-host-reclaim",
            run: stado::host_reclaim::run,
        },
        Spec {
            surface: "tui",
            title: "stado-release-doctor",
            run: stado::release_doctor::run,
        },
        Spec {
            surface: "tui",
            title: "stado-release-logs",
            run: stado::release_logs::run,
        },
        Spec {
            surface: "tui",
            title: "stado-release-quarantine",
            run: stado::release_quarantine::run,
        },
        Spec {
            surface: "tui",
            title: "stado-service-converge",
            run: stado::service_converge::run,
        },
        Spec {
            surface: "tui",
            title: "stado-service-ensure",
            run: stado::service_ensure::run,
        },
        Spec {
            surface: "tui",
            title: "stado-service-unowned-processes",
            run: stado::service_unowned_processes::run,
        },
        Spec {
            surface: "tui",
            title: "stado-cli-docs",
            run: stado::cli_docs::run,
        },
        Spec {
            surface: "tui",
            title: "stado-journeys",
            run: stado::journeys::run,
        },
        Spec {
            surface: "tui",
            title: "wisent-backend-production-latency",
            run: wisent::backend_production_latency::run,
        },
    ]
}
mod adam;
mod brama;
mod deep;
mod echo;
mod jeden;
mod las;
mod most;
mod oko;
mod singularity;
mod skarbiec;
mod skrzynka;
mod stado;
mod ugc;
mod weles;
mod wisent;
