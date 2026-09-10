use serde_json::json;
use crate::apphooks::*;
pub(crate) fn resolve_skarbiec(value: &mut Value, binary: &str) -> Result<(), Failure> {
    match value {
        Value::String(text) if text.starts_with("skarbiec://") => {
            let reference = text.trim_start_matches("skarbiec://");
            let (item, field) = reference.split_once('/').ok_or_else(|| {
                Failure::config(
                    "apphook.gac.config",
                    format!("invalid Skarbiec reference: {text}"),
                )
            })?;
            let output = Command::new(binary)
                .args(["get", item])
                .output()
                .map_err(|error| {
                    Failure::new(
                        "apphook.gac.skarbiec",
                        Code::Prerequisite,
                        format!("failed to run {binary}: {error}"),
                    )
                })?;
            if !output.status.success() {
                return Err(Failure::new(
                    "apphook.gac.skarbiec",
                    Code::Refused,
                    String::from_utf8_lossy(&output.stderr).trim().to_string(),
                ));
            }
            let document: Value = serde_json::from_slice(&output.stdout)?;
            let resolved = document
                .pointer(&format!("/fields/{field}"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    Failure::config(
                        "apphook.gac.skarbiec",
                        format!("Skarbiec item {item} has no field {field}"),
                    )
                })?;
            *text = resolved.to_string();
        }
        Value::Array(values) => {
            for value in values {
                resolve_skarbiec(value, binary)?;
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                resolve_skarbiec(value, binary)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn load_gac_config(path: &Path, environment: &BTreeMap<String, String>) -> Result<Value, Failure> {
    let mut value: Value = serde_json::from_slice(&fs::read(path).map_err(|error| {
        Failure::config("apphook.gac.config", format!("{}: {error}", path.display()))
    })?)?;
    let binary = environment
        .get("SKARBIEC_BIN")
        .map(String::as_str)
        .unwrap_or("skarbiec");
    resolve_skarbiec(&mut value, binary)?;
    Ok(value)
}

pub(crate) fn python_string(value: &Path) -> String {
    serde_json::to_string(&value.to_string_lossy()).expect("path string")
}

pub(crate) fn render_script(model: &Path, output: &Path, rotation: &str) -> String {
    [
        "import bpy".to_string(),
        "bpy.ops.wm.read_factory_settings(use_empty=True)".into(),
        format!("bpy.ops.import_scene.gltf(filepath={})", python_string(model)),
        "scene = bpy.context.scene".into(),
        format!("for obj in scene.objects: obj.rotation_euler = {rotation}"),
        "scene.render.engine = \"BLENDER_EEVEE_NEXT\" if hasattr(bpy.types, \"BLENDER_EEVEE_NEXT\") else \"BLENDER_EEVEE\"".into(),
        "scene.render.resolution_x = 512".into(),
        "scene.render.resolution_y = 512".into(),
        format!("scene.render.filepath = {}", python_string(output)),
        "bpy.ops.render.render(write_still=True)".into(),
        "import os".into(),
        format!("print(\"rendered\", os.path.getsize({}))", python_string(output)),
    ]
    .join("\n")
}

pub(crate) fn render_with_blender_session(
    root: &Path,
    mcp: &Value,
    scripts: &[String],
) -> Result<(), Failure> {
    let module = root.join("pipeline/blender.js");
    if !module.exists() {
        return Err(Failure::new(
            "apphook.gac.blender",
            Code::Prerequisite,
            format!(
                "game_asset_creator Blender dependency not found: {}",
                module.display()
            ),
        ));
    }
    const ADAPTER: &str = r#"import { pathToFileURL } from 'node:url';
pub(crate) const decode = value => Buffer.from(value, 'base64').toString('utf8');
pub(crate) const [modulePath, config, ...codes] = process.argv.slice(1);
pub(crate) const { BlenderSession } = await import(pathToFileURL(modulePath));
pub(crate) const session = await BlenderSession.start(JSON.parse(decode(config)));
try { for (const code of codes) await session.execute(decode(code)); }
finally { await session.close().catch(() => {}); }"#;
    let encode = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
    let mut command = Command::new("node");
    command.args(["--input-type=module", "--eval", ADAPTER]);
    command.arg(&module);
    command.arg(encode(serde_json::to_string(mcp)?.as_bytes()));
    for script in scripts {
        command.arg(encode(script.as_bytes()));
    }
    let output = command.output().map_err(|error| {
        Failure::new(
            "apphook.gac.blender",
            Code::Prerequisite,
            format!("failed to run node: {error}"),
        )
    })?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(Failure::new(
            "apphook.gac.blender",
            Code::Refused,
            if detail.is_empty() {
                format!("Blender session exited with {}", output.status)
            } else {
                detail
            },
        ));
    }
    Ok(())
}

pub(crate) fn score_with_brama(
    url: &str,
    key: &str,
    model: Option<&str>,
    rubric: &str,
    images: &[PathBuf],
) -> Result<Value, Failure> {
    let mut content = vec![json!({ "type": "text", "text": rubric })];
    for image in images {
        let png = fs::read(image)?;
        content.push(json!({
            "type": "image_url",
            "image_url": { "url": format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)) }
        }));
    }
    let endpoint = format!("{}/v1/chat/completions", url.trim_end_matches('/'));
    let body = request_json(
        "POST",
        &endpoint,
        &[
            ("content-type", "application/json".into()),
            ("authorization", format!("Bearer {key}")),
        ],
        Some(json!({
            "model": model.unwrap_or("any"),
            "max_tokens": 1024,
            "messages": [{ "role": "user", "content": content }]
        })),
    )
    .map_err(|failure| {
        if failure.detail.starts_with("HTTP ") {
            Failure::new(
                "apphook.gac.brama",
                Code::Refused,
                format!("brama {}", failure.detail),
            )
        } else {
            failure
        }
    })?;
    let text = body
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(start) = text.find('{') else {
        return Err(Failure::new(
            "apphook.gac.brama",
            Code::Refused,
            format!(
                "brama reply had no JSON: {}",
                text.chars().take(200).collect::<String>()
            ),
        ));
    };
    let Some(end) = text.rfind('}') else {
        return Err(Failure::new(
            "apphook.gac.brama",
            Code::Refused,
            format!(
                "brama reply had no JSON: {}",
                text.chars().take(200).collect::<String>()
            ),
        ));
    };
    serde_json::from_str(&text[start..=end]).map_err(Failure::from)
}
pub(crate) fn gac_root(harness: &Path, environment: &BTreeMap<String, String>) -> Result<PathBuf, Failure> {
    let configured = environment
        .get("GAC_ROOT")
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            crate::manifest::load(harness, "game-asset-creator").ok().and_then(|manifest| {
                manifest.document.get("repositories")
                    .and_then(serde_yaml::Value::as_sequence)
                    .and_then(|repositories| repositories.first())
                    .and_then(|repository| repository.get("root"))
                    .and_then(serde_yaml::Value::as_str)
                    .map(PathBuf::from)
            })
        })
        .ok_or_else(|| Failure::config(
            "apphook.gac.visual-eval",
            "GAC_ROOT is required: set it to the game_asset_creator repository or declare that repository first in apps/game-asset-creator/probierz.yaml",
        ))?;
    if !configured.is_dir() {
        return Err(Failure::new(
            "apphook.gac.visual-eval",
            Code::Prerequisite,
            format!(
                "game_asset_creator dependency not found at {}; set GAC_ROOT to the product repository",
                configured.display()
            ),
        ));
    }
    Ok(configured)
}

