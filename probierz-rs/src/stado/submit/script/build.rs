use crate::stado::*;
pub(crate) fn run_script(
    target: &str,
    app_id: &str,
    spec: Option<&str>,
    provision: Option<&Provision>,
    hash: &str,
    platform: Option<&str>,
    mode: &str,
    author: Option<(&str, &str, &str, &str)>,
    model_router_url: Option<&str>,
    record: bool,
    environment: &[(String, String)],
) -> Result<String, Failure> {
    let mut lines = vec![
        "set -euo pipefail".to_string(),
        "JOB_ROOT=\"$PWD\"".to_string(),
        "mkdir -p output work".to_string(),
        "export TMPDIR=\"$JOB_ROOT/work\"".to_string(),
        "export CARGO_HOME=\"${CARGO_HOME:-$HOME/.cargo}\"".to_string(),
        "export RUSTUP_HOME=\"${RUSTUP_HOME:-$HOME/.rustup}\"".to_string(),
        "export PATH=\"$CARGO_HOME/bin:$PATH\"".to_string(),
    ];
    if platform == Some("darwin") {
        lines.extend([
            "export PATH=$HOME/.stado/bin:$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH".into(),
            format!(
                "command -v node >/dev/null 2>&1 || {{ curl -fsSL https://nodejs.org/dist/{NODE_VERSION}/node-{NODE_VERSION}-darwin-arm64.tar.gz -o \"$TMPDIR/node.tar.gz\" && tar -xzf \"$TMPDIR/node.tar.gz\" -C \"$TMPDIR\" && export PATH=\"$TMPDIR/node-{NODE_VERSION}-darwin-arm64/bin:$PATH\"; }}",
            ),
        ]);
    } else if platform == Some("linux") {
        lines.extend([
            format!("curl -fsSL https://nodejs.org/dist/{NODE_VERSION}/node-{NODE_VERSION}-linux-x64.tar.xz -o \"$TMPDIR/node.tar.xz\""),
            "tar -xJf \"$TMPDIR/node.tar.xz\" -C \"$TMPDIR\"".into(),
            format!("export PATH=\"$TMPDIR/node-{NODE_VERSION}-linux-x64/bin:$PATH\""),
        ]);
    } else {
        lines.extend([
            "readonly PROBIERZ_WORKER_OS=\"$(uname -s)\"".into(),
            "readonly PROBIERZ_WORKER_ARCH=\"$(uname -m)\"".into(),
            "case \"$PROBIERZ_WORKER_OS:$PROBIERZ_WORKER_ARCH\" in".into(),
            "  Darwin:arm64) PROBIERZ_NODE_PLATFORM=darwin-arm64; PROBIERZ_NODE_EXTENSION=tar.gz ;;".into(),
            "  Darwin:x86_64) PROBIERZ_NODE_PLATFORM=darwin-x64; PROBIERZ_NODE_EXTENSION=tar.gz ;;".into(),
            "  Linux:aarch64|Linux:arm64) PROBIERZ_NODE_PLATFORM=linux-arm64; PROBIERZ_NODE_EXTENSION=tar.xz ;;".into(),
            "  Linux:x86_64|Linux:amd64) PROBIERZ_NODE_PLATFORM=linux-x64; PROBIERZ_NODE_EXTENSION=tar.xz ;;".into(),
            "  *) printf 'Unsupported Stado worker OS/architecture: %s/%s (supported: Darwin or Linux on arm64 or x64)\\n' \"$PROBIERZ_WORKER_OS\" \"$PROBIERZ_WORKER_ARCH\" >&2; exit 1 ;;".into(),
            "esac".into(),
            "if [ \"$PROBIERZ_WORKER_OS\" = Darwin ]; then export PATH=$HOME/.stado/bin:$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH; fi".into(),
            "if ! command -v node >/dev/null 2>&1; then".into(),
            format!("  PROBIERZ_NODE_ARCHIVE=\"node-{NODE_VERSION}-$PROBIERZ_NODE_PLATFORM.$PROBIERZ_NODE_EXTENSION\""),
            format!("  curl -fsSL \"https://nodejs.org/dist/{NODE_VERSION}/$PROBIERZ_NODE_ARCHIVE\" -o \"$TMPDIR/$PROBIERZ_NODE_ARCHIVE\""),
            "  case \"$PROBIERZ_NODE_EXTENSION\" in".into(),
            "    tar.gz) tar -xzf \"$TMPDIR/$PROBIERZ_NODE_ARCHIVE\" -C \"$TMPDIR\" ;;".into(),
            "    tar.xz) tar -xJf \"$TMPDIR/$PROBIERZ_NODE_ARCHIVE\" -C \"$TMPDIR\" ;;".into(),
            "  esac".into(),
            format!("  export PATH=\"$TMPDIR/node-{NODE_VERSION}-$PROBIERZ_NODE_PLATFORM/bin:$PATH\""),
            "fi".into(),
        ]);
    }
    lines.extend([
        "mkdir -p \"$JOB_ROOT/work/probierz\" && tar --no-same-owner -xzf \"$JOB_ROOT/inputs/probierz.tar.gz\" -C \"$JOB_ROOT/work/probierz\"".into(),
        "command -v cargo >/dev/null 2>&1 || { curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal; }".into(),
        "cargo build --locked --release --manifest-path \"$JOB_ROOT/work/probierz/probierz-rs/Cargo.toml\" --bin probierz".into(),
        "PROBIERZ=\"$JOB_ROOT/work/probierz/probierz-rs/target/release/probierz\"".into(),
        "HARNESS=\"$JOB_ROOT/work/probierz\"".into(),
    ]);
    if let Some(provision) = provision {
        match provision {
            Provision::InstalledTui { path, .. } => {
                lines.push(format!(
                    "export TUI_CMD={}",
                    shell_quote(&path.display().to_string())
                ));
            }
            Provision::NativeBinary { app_id, .. } => {
                lines.extend([
                    format!("mkdir -p \"$JOB_ROOT/work/{app_id}\" && tar --no-same-owner -xzf \"$JOB_ROOT/inputs/{app_id}.tar.gz\" -C \"$JOB_ROOT/work/{app_id}\""),
                    format!("export PROBIERZ_APP_SOURCE=\"$JOB_ROOT/work/{app_id}\""),
                    format!("cp \"$JOB_ROOT/inputs/{app_id}.binary\" \"$JOB_ROOT/work/{app_id}-binary\""),
                    format!("chmod 0755 \"$JOB_ROOT/work/{app_id}-binary\""),
                    format!("export TUI_CMD=\"$JOB_ROOT/work/{app_id}-binary\""),
                    "export PROBIERZ_BUILD_PATH=\"$TUI_CMD\"".into(),
                ]);
            }
            Provision::CargoRelease {
                app_id,
                binary,
                manifest_path,
            } => {
                let manifest_dir = Path::new(manifest_path)
                    .parent()
                    .filter(|path| !path.as_os_str().is_empty())
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| ".".to_string());
                let target_prefix = if manifest_dir == "." {
                    String::new()
                } else {
                    format!("{manifest_dir}/")
                };
                lines.extend([
                    format!("mkdir -p \"$JOB_ROOT/work/{app_id}\" && tar --no-same-owner -xzf \"$JOB_ROOT/inputs/{app_id}.tar.gz\" -C \"$JOB_ROOT/work/{app_id}\""),
                    format!("export PROBIERZ_APP_SOURCE=\"$JOB_ROOT/work/{app_id}\""),
                    format!("readonly PROBIERZ_CARGO_TARGET_DIR=\"$PROBIERZ_APP_SOURCE/{target_prefix}target\""),
                    "export CARGO_TARGET_DIR=\"$PROBIERZ_CARGO_TARGET_DIR\"".into(),
                    "trap 'rm -rf -- \"$PROBIERZ_CARGO_TARGET_DIR\"' EXIT".into(),
                    "command -v cargo >/dev/null 2>&1 || { curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal; }".into(),
                    format!("(cd \"$PROBIERZ_APP_SOURCE/{manifest_dir}\" && cargo build --locked --release --bins)"),
                    format!("export TUI_CMD=\"$JOB_ROOT/work/{app_id}/{target_prefix}target/release/{binary}\""),
                ]);
            }
            Provision::AppBundle {
                app_id,
                bundle_name,
                ..
            } => {
                let name = bundle_name.as_deref().ok_or_else(|| {
                    Failure::config("stado.pack", "application bundle was not staged")
                })?;
                lines.extend([
                    format!("mkdir -p \"$JOB_ROOT/work/{app_id}\" && tar --no-same-owner -xzf \"$JOB_ROOT/inputs/{app_id}-app.tar.gz\" -C \"$JOB_ROOT/work/{app_id}\""),
                    format!("export MAC_APP_PATH=\"$JOB_ROOT/work/{app_id}/{name}\""),
                    format!("mkdir -p \"$JOB_ROOT/work/{app_id}-src\" && tar --no-same-owner -xzf \"$JOB_ROOT/inputs/{app_id}.tar.gz\" -C \"$JOB_ROOT/work/{app_id}-src\""),
                    format!("export PROBIERZ_APP_SOURCE=\"$JOB_ROOT/work/{app_id}-src\""),
                ]);
                if target == "desktop:cua" {
                    lines.extend([
                        "CUA_EXECUTABLE=$(/usr/libexec/PlistBuddy -c \"Print :CFBundleExecutable\" \"$MAC_APP_PATH/Contents/Info.plist\")".into(),
                        "export CUA_APP_EXECUTABLE=\"$MAC_APP_PATH/Contents/MacOS/$CUA_EXECUTABLE\"".into(),
                    ]);
                }
            }
            Provision::NodeSource { app_id, .. } => {
                lines.extend([
                    format!("mkdir -p \"$JOB_ROOT/work/{app_id}\" && tar --no-same-owner -xzf \"$JOB_ROOT/inputs/{app_id}.tar.gz\" -C \"$JOB_ROOT/work/{app_id}\""),
                    format!("export PROBIERZ_APP_SOURCE=\"$JOB_ROOT/work/{app_id}\""),
                ]);
            }
        }
    }
    if mode == "author" && matches!(provision, None | Some(Provision::InstalledTui { .. })) {
        lines.extend([
            format!("mkdir -p \"$JOB_ROOT/work/{app_id}\" && tar --no-same-owner -xzf \"$JOB_ROOT/inputs/{app_id}.tar.gz\" -C \"$JOB_ROOT/work/{app_id}\""),
            format!("export PROBIERZ_APP_SOURCE=\"$JOB_ROOT/work/{app_id}\""),
        ]);
    }
    for (name, value) in environment {
        lines.push(format!("export {name}={}", shell_quote(value)));
    }
    script_body(
        target,
        app_id,
        hash,
        spec,
        provision,
        mode,
        author,
        model_router_url,
        record,
        environment,
        &mut lines,
    )?;
    Ok(lines.join("\n"))
}
