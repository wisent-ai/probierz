#!/bin/sh
set -eu

PATH="$HOME/.local/bin:$HOME/.stado/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"
export PATH
log="$HOME/.stado/logs/com.wisent.compute.agent.charless-mac-mini.log"
python3 - "$log" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
print(f"log={path}")
if not path.is_file():
    print("state=missing")
    raise SystemExit(0)
lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
print(f"state=present lines={len(lines)}")
for line in lines:
    if any(job_id in line for job_id in ("3aad13a2", "b340d8d6", "0f495349", "cff37ab4")):
        print(line[:500])
PY
plist="$HOME/Library/LaunchAgents/com.wisent.compute.agent.charless-mac-mini.plist"
python3 - "$plist" <<'PY'
from pathlib import Path
import json
import plistlib
import subprocess
import sys

path = Path(sys.argv[1])
if not path.is_file():
    print(f"plist={path} state=missing")
    raise SystemExit(0)
with path.open("rb") as handle:
    document = plistlib.load(handle)
environment = document.get("EnvironmentVariables") or {}
print(f"plist={path}")
for key in ("STADO_TARGET", "WC_STORAGE_BACKEND", "WC_LOCAL_STORAGE_PATH", "STADO_API_URL", "WC_AGENT_SKARBIEC_URL", "WC_AGENT_SKARBIEC_CONSUMER"):
    print(f"{key}={environment.get(key, '<unset>')}")
print(f"WC_AGENT_SKARBIEC_ITEMS.count={len(environment.get('WC_AGENT_SKARBIEC_ITEMS', '').split(','))}")
processes = subprocess.run(
    ["/bin/ps", "ax", "-o", "pid=", "-o", "ppid=", "-o", "etime=", "-o", "command="],
    check=True,
    capture_output=True,
    text=True,
).stdout.splitlines()
for process in processes:
    if "/.stado/bin/stado agent" in process:
        print(f"agent_process={process.strip()}")
configured = json.loads(subprocess.run(
    [str(Path.home() / ".stado/bin/stado"), "config", "show"],
    check=True,
    capture_output=True,
    text=True,
).stdout)["resolved"]
for key in ("wc_storage_backend", "wc_local_storage_path", "stado_api_url"):
    print(f"resolved.{key}={configured.get(key, '<unset>')}")
raw_config = json.loads((Path.home() / ".config/stado/config.json").read_text())
storage = raw_config.get("storage", {}).get("stado", {})
for key in ("url", "token_file", "namespace", "ca_file"):
    print(f"storage.stado.{key}={storage.get(key, '<unset>')}")
queue = subprocess.run(
    [str(Path.home() / ".stado/bin/stado"), "storage", "ls", "queue"],
    capture_output=True,
    text=True,
)
print(f"queue.exit={queue.returncode}")
print((queue.stdout or queue.stderr).strip()[:500])
PY
