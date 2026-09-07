#!/bin/sh
set -eu
PATH="$HOME/.local/bin:$HOME/.stado/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"
export PATH
python3 <<'PY'
import os
import signal
import subprocess
from pathlib import Path

expected = f"{Path.home()}/.stado/bin/stado agent --auto"
lines = subprocess.run(
    ["/bin/ps", "ax", "-o", "pid=", "-o", "command="],
    check=True,
    capture_output=True,
    text=True,
).stdout.splitlines()
processes = []
for line in lines:
    fields = line.strip().split(maxsplit=1)
    if len(fields) == 2 and fields[1] == expected:
        processes.append(int(fields[0]))
if not processes:
    raise SystemExit("no exact Stado local agent process is running")
keep = max(processes)
terminated = []
for pid in processes:
    if pid == keep:
        continue
    observed = subprocess.run(
        ["/bin/ps", "-p", str(pid), "-o", "command="],
        check=False,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if observed != expected:
        raise SystemExit(f"refusing to signal changed process {pid}: {observed!r}")
    os.kill(pid, signal.SIGTERM)
    terminated.append(pid)
print(f"kept={keep} terminated={','.join(map(str, terminated)) or '<none>'}")
PY
