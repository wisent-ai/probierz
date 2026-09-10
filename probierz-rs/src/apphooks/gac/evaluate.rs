use serde_json::json;
use crate::apphooks::*;
pub(crate) fn gac_visual_eval(
    harness: &Path,
    args: &[String],
    environment: &BTreeMap<String, String>,
) -> Result<Value, Failure> {
    let options = visual_options(args)?;
    let root = gac_root(harness, environment)?;
    let models = options
        .models
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("assets/models"));
    let output = options
        .out
        .map(PathBuf::from)
        .or_else(|| {
            environment
                .get("PROBIERZ_ARTIFACTS")
                .filter(|value| !value.trim().is_empty())
                .map(|root| Path::new(root).join("visual-eval"))
        })
        .unwrap_or_else(|| harness.join("test-results/visual-eval"));
    let config_path = options
        .config
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("pipeline.config.json"));
    let rubric_name = options
        .rubric
        .or_else(|| environment.get("PROBIERZ_EVAL_RUBRIC").cloned())
        .unwrap_or_else(|| "rts-character".into());
    let threshold = options
        .threshold
        .as_deref()
        .unwrap_or("0.7")
        .parse::<f64>()
        .map_err(|_| Failure::invalid("apphook.gac.visual-eval", "--threshold needs a number"))?;
    let config = load_gac_config(&config_path, environment)?;
    let brama = config.pointer("/models/brama").and_then(Value::as_object);
    let url = brama
        .and_then(|value| value.get("url"))
        .and_then(Value::as_str);
    let key = brama
        .and_then(|value| value.get("key"))
        .and_then(Value::as_str);
    if url.is_none() || key.is_none() {
        return Err(Failure::config(
            "apphook.gac.visual-eval",
            "models.brama.{url,key} missing from pipeline config (skarbiec:// refs)",
        ));
    }
    let mut glbs: Vec<PathBuf> = fs::read_dir(&models)
        .map_err(|error| {
            Failure::config(
                "apphook.gac.visual-eval",
                format!("{}: {error}", models.display()),
            )
        })?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("glb"))
        .collect();
    glbs.sort();
    if glbs.is_empty() {
        return Err(Failure::config(
            "apphook.gac.visual-eval",
            format!("no .glb files in {}", models.display()),
        ));
    }
    fs::create_dir_all(&output)?;
    let rubric = if rubric_name == "rts-character" {
        "You are an art director reviewing ONE low-poly RTS character render set.\nScore each dimension 0..1 and give an overall score 0..1:\n- proportions: chunky heroic low-poly (Thronefall style), not noodle-limbed\n- silhouette: readable at RTS camera distance, clear head/torso/weapon shapes\n- palette: flat-shaded colors consistent with a fantasy race (no texture noise)\n- artifacts: no z-fighting, no missing limbs, no collapsed geometry\nReply with a single JSON object: {\"proportions\": x, \"silhouette\": x, \"palette\": x, \"artifacts\": x, \"overall\": x, \"issues\": [\"...\"]}"
    } else {
        rubric_name.as_str()
    };
    let angles = [
        ("front", "(0, 0, 0)"),
        ("side", "(0, 0, 1.5708)"),
        ("back34", "(0, 0, 3.927)"),
    ];
    let empty_mcp = json!({});
    let mcp = config.pointer("/blender/mcp").unwrap_or(&empty_mcp);
    let mut results = Vec::new();
    for model_path in glbs {
        let stem = model_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("model");
        let renders: Vec<PathBuf> = angles
            .iter()
            .map(|(name, _)| output.join(format!("{stem}-{name}.png")))
            .collect();
        let scripts: Vec<String> = angles
            .iter()
            .zip(&renders)
            .map(|((_, rotation), render)| render_script(&model_path, render, rotation))
            .collect();
        render_with_blender_session(&root, mcp, &scripts)?;
        let scores = score_with_brama(
            url.expect("checked"),
            key.expect("checked"),
            brama
                .and_then(|value| value.get("model"))
                .and_then(Value::as_str),
            rubric,
            &renders,
        )?;
        let passed = scores
            .get("overall")
            .and_then(Value::as_f64)
            .is_some_and(|value| value >= threshold);
        results.push(json!({
            "asset": model_path.file_name().and_then(|value| value.to_str()).unwrap_or(""),
            "renders": renders,
            "scores": scores,
            "pass": passed
        }));
    }
    let passed = results
        .iter()
        .filter(|result| result.get("pass").and_then(Value::as_bool) == Some(true))
        .count();
    let report = json!({
        "rubric": rubric_name,
        "threshold": threshold,
        "total": results.len(),
        "passed": passed,
        "failed": results.len() - passed,
        "results": results
    });
    let report_path = output.join("eval-report.json");
    write_private(&report_path, &serde_json::to_vec_pretty(&report)?)?;
    if passed != results.len() {
        return Err(Failure::new(
            "apphook.gac.visual-eval",
            Code::Refused,
            format!(
                "visual evaluation failed: {} of {} assets failed; report: {}",
                results.len() - passed,
                results.len(),
                report_path.display()
            ),
        ));
    }
    Ok(
        json!({ "reportPath": report_path, "total": results.len(), "passed": passed, "failed": results.len() - passed }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_ids_and_fixture_shape_match_the_javascript_contract() {
        assert_eq!(
            deterministic_uuid(&["oko-e2e-org", "run-123"]),
            "dc61a7ae-b9d2-4bd0-8864-0783fa068918"
        );
        let strategy = strategy_document("run-123");
        assert_eq!(
            strategy["northStar"]["metrics"].as_array().map(Vec::len),
            Some(4)
        );
        assert_eq!(strategy["initiatives"].as_array().map(Vec::len), Some(13));
        let capacity: f64 = strategy["initiatives"]
            .as_array()
            .expect("initiatives")
            .iter()
            .filter_map(|item| item.get("capacityPercent").and_then(Value::as_f64))
            .sum();
        assert!((capacity - 100.0).abs() < 0.0001);
    }

    #[test]
    fn fixture_builder_writes_real_glb_headers_and_private_config() {
        let harness =
            std::env::temp_dir().join(format!("probierz-gac-hook-{}", std::process::id()));
        let result = gac_fixtures(&harness, &BTreeMap::new()).expect("fixtures");
        let directory = harness.join("test-results/game-asset-creator/fixtures");
        assert_eq!(result["dir"].as_str(), directory.to_str());
        let valid = fs::read(result["valid"].as_str().expect("valid path")).expect("valid GLB");
        assert_eq!(&valid[..4], b"glTF");
        assert_eq!(
            u32::from_le_bytes(valid[4..8].try_into().expect("version")),
            2
        );
        assert_eq!(
            u32::from_le_bytes(valid[8..12].try_into().expect("length")) as usize,
            valid.len()
        );
        assert_eq!(
            fs::read(result["corrupt"].as_str().expect("corrupt path")).expect("corrupt"),
            b"definitely not a glb file"
        );
        fs::remove_dir_all(harness).expect("cleanup");
    }

    #[test]
    fn autonomy_only_seed_refuses_no_external_dependency() {
        let environment =
            BTreeMap::from([("PROBIERZ_JOURNEYS".into(), "autonomy-experimental".into())]);
        assert_eq!(
            oko_seed(&environment).expect("skip"),
            json!({ "skipped": "autonomy journey uses isolated local fixtures" })
        );
    }
}
