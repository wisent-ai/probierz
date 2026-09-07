//! The journeys that run against terminal applications, over a real PTY.

use super::Spec;

mod common;
mod echo_common;

mod adam_agent_toolkit_onboarding_first_use;
mod adam_services_onboarding_first_use;
mod brama_onboarding_first_use;
mod brama_subscription_pool;
mod deep_analytics_onboarding_first_use;
mod echo_docs_capabilities;
mod echo_read_every_capability;
mod echo_web_analytics_collectors;
mod echo_web_capability_catalogue;
mod game_asset_creator_cli;
mod jeden_model_routing;
mod jeden_onboarding_first_use;
mod jeden_settings_screen;
mod jeden_task_contract_lifecycle;
mod las_onboarding_first_use;
mod most_onboarding_first_use;
mod oko_autonomy;
mod singularity_onboarding_first_use;
mod skarbiec_delete_and_restore_secret;
mod skarbiec_fixture;
mod skarbiec_initialize_vault;
mod skarbiec_manage_secrets;
mod skarbiec_manage_service_routes;
mod skarbiec_manage_users_and_sharing;
mod skarbiec_onboarding_first_use;
mod skarbiec_recover_vault_access;
mod skarbiec_use_json_output;
mod skrzynka_crate_builds;
mod skrzynka_mailbox_lifecycle;
mod skrzynka_oauth_failure_diagnosis;
mod ssh_auth_router_onboarding_first_use;
mod stado_fleet_fixture;
mod stado_host_gates;
mod stado_host_reclaim;
mod stado_release_doctor;
mod stado_release_logs;
mod stado_release_quarantine;
mod stado_service_converge;
mod stado_service_ensure;
mod stado_service_unowned_processes;
mod ugc_cli_onboarding_first_use;
mod weles_onboarding_first_use;
mod wisent_benchmark_onboarding_first_use;
mod wisent_onboarding_first_use;
mod wisent_optimizer_onboarding_first_use;
mod stado_cli_docs;
mod stado_journeys;
mod wisent_backend_production_latency;

/// Every journey registered for this surface.
pub fn specs() -> Vec<Spec> {
    vec![
        Spec {
            surface: "tui",
            title: "adam-agent-toolkit-onboarding-first-use",
            run: adam_agent_toolkit_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "adam-services-onboarding-first-use",
            run: adam_services_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "brama-onboarding-first-use",
            run: brama_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "brama-subscription-pool",
            run: brama_subscription_pool::run,
        },
        Spec {
            surface: "tui",
            title: "deep-analytics-onboarding-first-use",
            run: deep_analytics_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "echo-docs-capabilities",
            run: echo_docs_capabilities::run,
        },
        Spec {
            surface: "tui",
            title: "echo-read-every-capability",
            run: echo_read_every_capability::run,
        },
        Spec {
            surface: "tui",
            title: "echo-web-analytics-collectors",
            run: echo_web_analytics_collectors::run,
        },
        Spec {
            surface: "tui",
            title: "echo-web-capability-catalogue",
            run: echo_web_capability_catalogue::run,
        },
        Spec {
            surface: "tui",
            title: "game-asset-creator-cli",
            run: game_asset_creator_cli::run,
        },
        Spec {
            surface: "tui",
            title: "jeden-model-routing",
            run: jeden_model_routing::run,
        },
        Spec {
            surface: "tui",
            title: "jeden-onboarding-first-use",
            run: jeden_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "jeden-settings-screen",
            run: jeden_settings_screen::run,
        },
        Spec {
            surface: "tui",
            title: "jeden-task-contract-lifecycle",
            run: jeden_task_contract_lifecycle::run,
        },
        Spec {
            surface: "tui",
            title: "las-onboarding-first-use",
            run: las_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "most-onboarding-first-use",
            run: most_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "oko-autonomy",
            run: oko_autonomy::run,
        },
        Spec {
            surface: "tui",
            title: "singularity-onboarding-first-use",
            run: singularity_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "skrzynka-crate-builds",
            run: skrzynka_crate_builds::run,
        },
        Spec {
            surface: "tui",
            title: "skrzynka-mailbox-lifecycle",
            run: skrzynka_mailbox_lifecycle::run,
        },
        Spec {
            surface: "tui",
            title: "skrzynka-oauth-failure-diagnosis",
            run: skrzynka_oauth_failure_diagnosis::run,
        },
        Spec {
            surface: "tui",
            title: "ssh-auth-router-onboarding-first-use",
            run: ssh_auth_router_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "ugc-cli-onboarding-first-use",
            run: ugc_cli_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "weles-onboarding-first-use",
            run: weles_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "wisent-benchmark-onboarding-first-use",
            run: wisent_benchmark_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "wisent-onboarding-first-use",
            run: wisent_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "wisent-optimizer-onboarding-first-use",
            run: wisent_optimizer_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "skarbiec-delete-and-restore-secret",
            run: skarbiec_delete_and_restore_secret::run,
        },
        Spec {
            surface: "tui",
            title: "skarbiec-initialize-vault",
            run: skarbiec_initialize_vault::run,
        },
        Spec {
            surface: "tui",
            title: "skarbiec-manage-secrets",
            run: skarbiec_manage_secrets::run,
        },
        Spec {
            surface: "tui",
            title: "skarbiec-manage-service-routes",
            run: skarbiec_manage_service_routes::run,
        },
        Spec {
            surface: "tui",
            title: "skarbiec-manage-users-and-sharing",
            run: skarbiec_manage_users_and_sharing::run,
        },
        Spec {
            surface: "tui",
            title: "skarbiec-onboarding-first-use",
            run: skarbiec_onboarding_first_use::run,
        },
        Spec {
            surface: "tui",
            title: "skarbiec-recover-vault-access",
            run: skarbiec_recover_vault_access::run,
        },
        Spec {
            surface: "tui",
            title: "skarbiec-use-json-output",
            run: skarbiec_use_json_output::run,
        },
        Spec {
            surface: "tui",
            title: "stado-host-gates",
            run: stado_host_gates::run,
        },
        Spec {
            surface: "tui",
            title: "stado-host-reclaim",
            run: stado_host_reclaim::run,
        },
        Spec {
            surface: "tui",
            title: "stado-release-doctor",
            run: stado_release_doctor::run,
        },
        Spec {
            surface: "tui",
            title: "stado-release-logs",
            run: stado_release_logs::run,
        },
        Spec {
            surface: "tui",
            title: "stado-release-quarantine",
            run: stado_release_quarantine::run,
        },
        Spec {
            surface: "tui",
            title: "stado-service-converge",
            run: stado_service_converge::run,
        },
        Spec {
            surface: "tui",
            title: "stado-service-ensure",
            run: stado_service_ensure::run,
        },
        Spec {
            surface: "tui",
            title: "stado-service-unowned-processes",
            run: stado_service_unowned_processes::run,
        },
        Spec {
            surface: "tui",
            title: "stado-cli-docs",
            run: stado_cli_docs::run,
        },
        Spec {
            surface: "tui",
            title: "stado-journeys",
            run: stado_journeys::run,
        },
        Spec {
            surface: "tui",
            title: "wisent-backend-production-latency",
            run: wisent_backend_production_latency::run,
        },
    ]
}
