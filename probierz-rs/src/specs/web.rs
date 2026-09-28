//! The journeys that run against web pages, through a Weles browser.

use super::Spec;

mod landing;
mod onboarding;
pub(crate) mod weles;

/// Every journey registered for this surface.
pub fn specs() -> Vec<Spec> {
    vec![
        Spec {
            surface: "web",
            title: "adam-monitor-onboarding-first-use",
            run: onboarding::dashboards::adam_monitor,
        },
        Spec {
            surface: "web",
            title: "compute-marketplace-onboarding-first-use",
            run: onboarding::dashboards::compute_marketplace,
        },
        Spec {
            surface: "web",
            title: "landing-page-release-evaluation",
            run: landing::release_evaluation,
        },
        Spec {
            surface: "web",
            title: "skarbiec-hub-onboarding-first-use",
            run: onboarding::results::skarbiec_hub,
        },
        Spec {
            surface: "web",
            title: "weles-console-onboarding-first-use",
            run: onboarding::accounts::weles_console,
        },
        Spec {
            surface: "web",
            title: "wisent-app-onboarding-first-use",
            run: onboarding::accounts::wisent_app,
        },
        Spec {
            surface: "web",
            title: "wisent-gradio-onboarding-first-use",
            run: onboarding::results::wisent_gradio,
        },
        Spec {
            surface: "web",
            title: "wisent-trade-onboarding-first-use",
            run: onboarding::dashboards::wisent_trade,
        },
    ]
}
