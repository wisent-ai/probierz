use crate::apphooks::*;
use serde_json::json;
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
    // The rubric is the product's declaration, not text held here:
    // apps/game-asset-creator/rubrics/<name>.json states the rubric and the
    // overall score an asset must reach; --threshold overrides that score for
    // one run. A run naming no rubric, a name that is not one file name, a
    // name declared nowhere and a declaration without its text or score are
    // refused by name.
    let point = "apphook.gac.visual-eval";
    let rubric_name = options
        .rubric
        .or_else(|| environment.get("PROBIERZ_EVAL_RUBRIC").cloned())
        .ok_or_else(|| {
            Failure::invalid(
                point,
                "visual-eval needs --rubric <name> or PROBIERZ_EVAL_RUBRIC",
            )
        })?;
    if Path::new(&rubric_name)
        .file_name()
        .and_then(|name| name.to_str())
        != Some(rubric_name.as_str())
    {
        return Err(Failure::invalid(
            point,
            format!("rubric name {rubric_name:?} is not one file name"),
        ));
    }
    let declaration_path = harness
        .join("apps/game-asset-creator/rubrics")
        .join(format!("{rubric_name}.json"));
    let declaration = crate::json_file::read(&declaration_path)?.ok_or_else(|| {
        Failure::config(
            point,
            format!(
                "rubric {rubric_name} is declared nowhere: {} does not exist",
                declaration_path.display()
            ),
        )
    })?;
    let rubric = declaration
        .get("rubric")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Failure::config(
                point,
                format!("{} states no rubric text", declaration_path.display()),
            )
        })?
        .to_string();
    let threshold = match options.threshold.as_deref() {
        Some(stated) => stated
            .parse::<f64>()
            .map_err(|_| Failure::invalid(point, "--threshold needs a number"))?,
        None => declaration
            .get("threshold")
            .and_then(Value::as_f64)
            .ok_or_else(|| {
                Failure::config(
                    point,
                    format!("{} states no threshold", declaration_path.display()),
                )
            })?,
    };
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
    // An entry that cannot be read is refused with the directory, not
    // skipped: skipped, its model was left out of the evaluation unnoticed.
    let listing_failed = |error: std::io::Error| {
        Failure::config(
            "apphook.gac.visual-eval",
            format!("{}: {error}", models.display()),
        )
    };
    let mut glbs: Vec<PathBuf> = fs::read_dir(&models)
        .map_err(listing_failed)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(listing_failed)?
        .into_iter()
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
    let rubric = rubric.as_str();
    let angles = [
        ("front", "(0, 0, 0)"),
        ("side", "(0, 0, 1.5708)"),
        ("back34", "(0, 0, 3.927)"),
    ];
    let empty_mcp = json!({});
    let mcp = config.pointer("/blender/mcp").unwrap_or(&empty_mcp);
    let mut results = Vec::new();
    for model_path in glbs {
        // Named from the model's own file stem: 'model' stood in for one that
        // is not Unicode, so two such models wrote over each other's renders.
        let stem = model_path
            .file_stem()
            .map(|value| value.to_string_lossy().into_owned())
            .ok_or_else(|| {
                Failure::config(
                    "apphook.gac.visual-eval",
                    format!("{} has no file stem", model_path.display()),
                )
            })?;
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
            "asset": model_path.file_name().map(|value| value.to_string_lossy()),
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
