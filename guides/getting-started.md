# Getting started

The supported source installation, the read-only discovery commands, and
adopting journey definitions from another Probierz repository. None of
this starts a browser, installs drivers, drives an application, or
creates run evidence.

## Prerequisites

- Git;
- a stable Rust toolchain with Cargo;
- a source checkout of the public repository.

```bash
git clone https://github.com/wisent-ai/probierz.git
cd probierz/probierz-rs
cargo build --release
export PATH="$PWD/target/release:$PATH"
probierz list
probierz apps
```

Expected result: `list` returns the web, Electron, mobile, and native-desktop
surfaces with their targets and environment requirements. `apps` returns the
validated application manifests currently registered in the checkout. Neither
command executes a test target.

## Adopt existing journey definitions

First setup can start from another Probierz repository without executing its
journeys:

```bash
node agent/cli.mjs onboarding --source /absolute/path/to/existing-probierz
# The same operation is reusable after onboarding:
node agent/cli.mjs project adopt --source /absolute/path/to/existing-probierz
node agent/cli.mjs project adoptions
```

The selected Git repository must contain validated
`apps/<appId>/probierz.yaml` manifests and their referenced specs in the
established `packages/<surface>/test/specs`, `tests`, or `specs` directories.
Probierz validates the complete selection before changing the destination,
copies definitions byte-for-byte with their file modes, and records the
canonical source path, content digest, applications, and per-file digests and
modes in `apps/.adoptions.json`. The source path plus content identity makes
repeat adoption idempotent.

Existing definitions are preserved by default. A differing destination returns
the full conflict list and writes nothing. After reviewing those paths, an
explicit `--replace` updates unmanaged files and previously adopted same-source
files that still match their retained content and mode. Definitions owned by
another imported source and locally changed same-source files remain conflicts,
including locally changed files that disappeared upstream. A source repository's
own `.adoptions.json` is reported and excluded as local history. Import never
runs a spec, creates evidence, installs tools, or changes the selected source
repository. Skipping this optional step leaves a usable empty project.

Desktop's Workspace screen distinguishes an empty adoption list from a failed
read and retains the previous list beside the concrete read error. Switching
workspaces discards an earlier workspace's in-flight list result or error;
canceled reads likewise cannot replace the selected workspace's state.
An accepted import remains accepted if the following adoption-list read fails:
the Workspace screen reports that read failure separately instead of claiming
that the already-persisted definitions were not adopted. Adoption callbacks
from a previously selected workspace do not overwrite the current screen.
