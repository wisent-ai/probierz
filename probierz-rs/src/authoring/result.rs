use crate::authoring::*;
pub fn print_result(result: JsonValue) -> Result<bool, Failure> {
    let ok = result
        .get("ok")
        .and_then(JsonValue::as_bool)
        .or_else(|| result.pointer("/verdict/pass").and_then(JsonValue::as_bool))
        .or_else(|| result.get("pass").and_then(JsonValue::as_bool))
        .unwrap_or(true);
    print_json(&result)?;
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn authoring_router_refuses_missing_url_exactly() {
        assert_eq!(
            stado_model_router_url(None).unwrap_err(),
            "STADO_MODEL_ROUTER_URL is required"
        );
        assert_eq!(
            stado_model_router_url(Some("   ")).unwrap_err(),
            "STADO_MODEL_ROUTER_URL is required"
        );
    }

    #[test]
    fn figure_refuses_missing_model_before_router() {
        let root = Path::new("/");
        assert_eq!(
            figure_prerequisites(root, None, None, Some("  "), Some("  ")).unwrap_err(),
            "--model or PROBIERZ_FIGURE_VISION_MODEL is required"
        );
    }

    #[test]
    fn seo_refuses_missing_models() {
        let root = Path::new("/");
        assert_eq!(
            seo_prerequisites(root, "missing", None, Some(" "), Some(" "), Some(" ")).unwrap_err(),
            "PROBIERZ_SEO_PRIMARY_MODEL is required"
        );
        assert_eq!(
            seo_prerequisites(root, "missing", None, Some("first"), Some(" "), Some(" "))
                .unwrap_err(),
            "PROBIERZ_SEO_SECONDARY_MODEL is required"
        );
    }

    #[test]
    fn router_rejects_unsafe_urls() {
        assert_eq!(
            stado_model_router_url(Some("http://example.com")).unwrap_err(),
            "STADO_MODEL_ROUTER_URL must use HTTPS or loopback HTTP"
        );
        assert_eq!(
            stado_model_router_url(Some("https://user@example.com?q=1")).unwrap_err(),
            "STADO_MODEL_ROUTER_URL must not contain credentials, query parameters, or a fragment"
        );
        assert_eq!(
            stado_model_router_url(Some("http://127.0.0.1:8080/")).unwrap(),
            "http://127.0.0.1:8080"
        );
    }

    #[test]
    fn repair_patch_refuses_protected_paths() {
        let patch = "diff --git a/.env b/.env\n--- a/.env\n+++ b/.env\n";
        assert_eq!(patch_paths(patch).unwrap_err(), "patch may not change .env");
    }

    #[test]
    fn repair_selects_latest_failed_run_not_latest_directory() {
        let harness = tempfile::tempdir().unwrap();
        for (directory, run_id, status, started) in [
            ("zzz", "passed-new", "passed", "2026-02-01T00:00:00.000Z"),
            ("aaa", "failed-old", "failed", "2026-01-01T00:00:00.000Z"),
        ] {
            let path = harness.path().join("test-results/demo/web").join(directory);
            fs::create_dir_all(&path).unwrap();
            fs::write(
                path.join("run-manifest.json"),
                serde_json::to_vec(&json!({
                    "runId": run_id, "appId": "demo", "target": "web", "status": status,
                    "startedAt": started, "completedAt": started
                }))
                .unwrap(),
            )
            .unwrap();
        }
        let run = repair_source_run(harness.path(), "demo", None).unwrap();
        assert_eq!(
            run.get("runId").and_then(JsonValue::as_str),
            Some("failed-old")
        );
    }
}
