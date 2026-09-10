use serde_json::json;
use crate::apphooks::*;
pub(crate) fn glb(model: Value) -> Result<Vec<u8>, Failure> {
    let mut source = serde_json::to_string(&model)?;
    while source.len() % 4 != 0 {
        source.push(' ');
    }
    let length = 12 + 8 + source.len();
    let mut bytes = Vec::with_capacity(length);
    bytes.extend_from_slice(b"glTF");
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&(length as u32).to_le_bytes());
    bytes.extend_from_slice(&(source.len() as u32).to_le_bytes());
    bytes.extend_from_slice(b"JSON");
    bytes.extend_from_slice(source.as_bytes());
    Ok(bytes)
}

pub(crate) fn model_json(triangles: usize) -> Value {
    json!({
        "asset": { "version": "2.0" },
        "accessors": [{ "count": triangles * 3 }],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "mode": 4 }] }],
        "materials": [{}]
    })
}

pub(crate) fn gac_fixtures(harness: &Path, source: &BTreeMap<String, String>) -> Result<Value, Failure> {
    let directory = source
        .get("GAC_FIXTURE_DIR")
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            source
                .get("PROBIERZ_ARTIFACTS")
                .filter(|value| !value.trim().is_empty())
                .map(|root| Path::new(root).join("fixtures/game-asset-creator"))
        })
        .unwrap_or_else(|| harness.join("test-results/game-asset-creator/fixtures"));
    fs::create_dir_all(&directory)?;
    let valid = directory.join("valid-6k.glb");
    let over_budget = directory.join("over-budget.glb");
    let corrupt = directory.join("corrupt.glb");
    let fake_skarbiec = directory.join("skarbiec");
    let config = directory.join("pipeline.config.json");
    fs::write(&valid, glb(model_json(6000))?)?;
    fs::write(&over_budget, glb(model_json(99_999))?)?;
    fs::write(&corrupt, b"definitely not a glb file")?;
    fs::write(
        &fake_skarbiec,
        b"#!/bin/sh\nif [ \"$1\" = \"get\" ]; then\n  case \"$2\" in\n    TEXT2GAME_ACCOUNT) echo '{\"fields\":{\"login_email\":\"fixture@example.com\",\"login_password\":\"fixture\"}}' ;;\n    BRAMA) echo '{\"fields\":{\"agent_auth_secret\":\"fixture-brama-key\"}}' ;;\n    *) echo \"item not found: $2\" >&2; exit 1 ;;\n  esac\n  exit 0\nfi\necho \"unknown command: $1\" >&2\nexit 1\n",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_skarbiec, fs::Permissions::from_mode(0o755))?;
    }
    let document = json!({
        "browser": { "headless": true },
        "credentials": {
            "username": "skarbiec://TEXT2GAME_ACCOUNT/login_email",
            "password": "skarbiec://TEXT2GAME_ACCOUNT/login_password"
        },
        "models": { "brama": {
            "url": "https://model-router.example",
            "key": "skarbiec://BRAMA/agent_auth_secret",
            "model": "any"
        }},
        "studio": {
            "loginUrl": "https://studio.example/login",
            "generateUrl": "https://studio.example/generate",
            "selectors": {
                "loginUser": "#u",
                "loginPassword": "#p",
                "loginSubmit": "#go",
                "promptInput": "#prompt",
                "generateSubmit": "#gen"
            },
            "artifact": { "pollExpression": "null", "timeoutMs": 1000, "intervalMs": 100 }
        },
        "verify": { "enabled": true, "triTarget": 6000, "triTolerancePct": 100 }
    });
    write_private(&config, &serde_json::to_vec_pretty(&document)?)?;
    Ok(json!({
        "dir": directory,
        "valid": valid,
        "overBudget": over_budget,
        "corrupt": corrupt,
        "fakeSkarbiec": fake_skarbiec,
        "config": config,
        "env": { "GAC_FIXTURE_DIR": directory }
    }))
}

#[derive(Default)]
pub(crate) struct VisualOptions {
    pub(crate) models: Option<String>,
    pub(crate) out: Option<String>,
    pub(crate) config: Option<String>,
    pub(crate) rubric: Option<String>,
    pub(crate) threshold: Option<String>,
}

pub(crate) fn visual_options(args: &[String]) -> Result<VisualOptions, Failure> {
    let mut options = VisualOptions::default();
    let mut index = 0;
    while index < args.len() {
        let name = args[index].strip_prefix("--").ok_or_else(|| {
            Failure::invalid(
                "apphook.gac.visual-eval",
                format!("unexpected argument: {}", args[index]),
            )
        })?;
        let value = args
            .get(index + 1)
            .ok_or_else(|| {
                Failure::invalid("apphook.gac.visual-eval", format!("--{name} needs a value"))
            })?
            .clone();
        match name {
            "models" => options.models = Some(value),
            "out" => options.out = Some(value),
            "config" => options.config = Some(value),
            "rubric" => options.rubric = Some(value),
            "threshold" => options.threshold = Some(value),
            other => {
                return Err(Failure::invalid(
                    "apphook.gac.visual-eval",
                    format!("unknown visual-eval option: --{other}"),
                ))
            }
        }
        index += 2;
    }
    Ok(options)
}

