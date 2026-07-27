#!/bin/bash
# sculpt-job.sh — game_asset_creator sculpt job for a stado worker.
#
# Runs AFTER the bridge provisioned node + unpacked:
#   /tmp/w/probierz            (this repo, we're cd'd into it)
#   /tmp/w/game_asset_creator  (app sources, $GAC_ROOT points here)
#   /tmp/game_asset_creator-resolved-config.json (submit-time vault-resolved
#                                                 config, mode 0600)
#
# Env expected from the bridge (--env): GAC_ROOT, SCULPT_PROMPT,
# optional SCULPT_FILENAME, SCULPT_ROUNDS, SCULPT_OUT.
#
# What it does: install Blender + uv (Linux), resolve blender-mcp, install
# its addon into Blender, start Blender with the addon's socket server,
# health-check, run the LLM sculpt, then copy artifacts to test-results/.

set -euxo pipefail

GAC_ROOT="${GAC_ROOT:-/tmp/w/game_asset_creator}"
RESOLVED_CONFIG="${RESOLVED_CONFIG:-/tmp/game_asset_creator-resolved-config.json}"
SCULPT_PROMPT="${SCULPT_PROMPT:-low-poly boulder, Thronefall style}"
SCULPT_FILENAME="${SCULPT_FILENAME:-sculpt-output.glb}"
SCULPT_ROUNDS="${SCULPT_ROUNDS:-12}"
SCULPT_OUT="${SCULPT_OUT:-/tmp/w/gac-out}"
RESULTS_DIR="${RESULTS_DIR:-test-results/sculpt-job}"

mkdir -p "$SCULPT_OUT" "$RESULTS_DIR"

# --- 1. Blender + uv (gac's own provisioning, Linux plan) ---
cd "$GAC_ROOT"
cp "$RESOLVED_CONFIG" pipeline.config.json
chmod 600 pipeline.config.json
node pipeline/setup.js || node pipeline/setup.js  # apt can be flaky once

# --- 2. blender-mcp addon into Blender ---
UVX="$(command -v uvx || echo "$HOME/.local/bin/uvx")"
"$UVX" --from blender-mcp blender-mcp --help || true
ADDON_PY="$("$UVX" --from blender-mcp python - <<'PY' 2>/dev/null || true
import importlib.util, pathlib
spec = importlib.util.find_spec("blender_mcp")
if spec and spec.origin:
    root = pathlib.Path(spec.origin).parent.parent
    cand = list(root.glob("**/addon.py"))
    print(cand[0] if cand else "")
PY
)"
if [ -z "$ADDON_PY" ]; then
  ADDON_PY="$(find "$HOME/.cache/uv" -name addon.py -path '*blender*' 2>/dev/null | head -1)"
fi
echo "addon.py: $ADDON_PY"

BLENDER_BIN="$(command -v blender)"
"$BLENDER_BIN" -b --python-expr "
import bpy, re
bpy.ops.preferences.addon_install(filepath='$ADDON_PY', overwrite=True)
# The upstream addon reads scene.blendermcp_use_* directly; a scene reset
# (or factory-settings call) wipes those and kills its server thread.
# Patch all reads to getattr-with-default BEFORE enabling (learned the hard way).
import addon as _a
src = _a.__file__
code = open(src).read()
for attr in ['blendermcp_use_polyhaven','blendermcp_use_hyper3d','blendermcp_use_sketchfab','blendermcp_use_hunyuan3d']:
    code = code.replace(f'bpy.context.scene.{attr}', f'getattr(bpy.context.scene, \"{attr}\", False)')
open(src, 'w').write(code)
bpy.ops.preferences.addon_enable(module='addon')
bpy.ops.wm.save_userpref()
print('addon installed+patched+enabled')
"

# --- 3. Blender with the addon's socket server, in the background ---
cat > /tmp/blender_mcp_bootstrap.py <<'PY'
import bpy, time, sys

def start_server():
    # Preferred: instantiate the addon's server class directly.
    try:
        import addon
        for attr in ("BlenderMCPServer", "BlenderMcpServer"):
            cls = getattr(addon, attr, None)
            if cls is not None:
                srv = cls()
                start = getattr(srv, "start", None) or getattr(srv, "execute", None)
                if start:
                    start()
                    print(f"mcp-server: started via addon.{attr}", flush=True)
                    return True
    except Exception as exc:
        print(f"mcp-server: addon class path failed: {exc}", flush=True)
    # Fallback: registered operators used by the addon panel.
    for op in ("connect", "start_server", "start"):
        fn = getattr(bpy.ops.blendermcp, op, None)
        if fn is not None:
            try:
                fn()
                print(f"mcp-server: started via bpy.ops.blendermcp.{op}", flush=True)
                return True
            except Exception as exc:
                print(f"mcp-server: bpy.ops.blendermcp.{op} failed: {exc}", flush=True)
    return False

ok = start_server()
sys.exit(0 if ok else 3)
PY
nohup "$BLENDER_BIN" -b --python /tmp/blender_mcp_bootstrap.py > /tmp/blender.log 2>&1 &
BLENDER_PID=$!
echo "blender pid: $BLENDER_PID"
sleep 10
tail -50 /tmp/blender.log || true

# --- 4. health + sculpt ---
node pipeline/cli.js blender-health
node pipeline/cli.js sculpt "$SCULPT_PROMPT" \
  --out "$SCULPT_OUT" \
  --filename "$SCULPT_FILENAME" \
  --rounds "$SCULPT_ROUNDS" | tee "$RESULTS_DIR/sculpt-result.json"

# --- 5. artifacts back ---
kill $BLENDER_PID || true
cp -r "$SCULPT_OUT" "$RESULTS_DIR/models" || true
node pipeline/cli.js verify "$SCULPT_OUT/$SCULPT_FILENAME" | tee "$RESULTS_DIR/verify-report.json" || true
echo "sculpt-job done"
