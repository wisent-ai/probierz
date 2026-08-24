# Walkthrough: register an application

Executed end-to-end on this checkout on 2026-08-24. Every output below is
pasted from the session (long absolute paths shortened to `<checkout>`).
The goal: take a product Probierz has never seen and register it so runs,
history, and gates can hold it accountable. Registration is one file — a
validated `apps/<appId>/probierz.yaml` — nothing else.

The runnable version of this walkthrough is
[examples/register-demo-app.sh](examples/register-demo-app.sh);
[examples/cleanup-demo-app.sh](examples/cleanup-demo-app.sh) removes
everything it creates.

## 1. The product

A one-prompt terminal program, the smallest thing with a user journey:

```bash
mkdir -p /tmp/probierz-demo-app && cd /tmp/probierz-demo-app && git init -q
cat > hello.mjs <<'EOF'
#!/usr/bin/env node
// Minimal interactive TUI: asks for a name, greets, exits 0.
import readline from "node:readline";
const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
rl.question("What is your name? ", (name) => {
  process.stdout.write(`Hello, ${name || "stranger"}!\n`);
  rl.close();
});
EOF
git add -A && git commit -qm "demo product"
```

The repository matters: app source identity is computed per declared
repository, and `status` freshness compares recorded SHAs against its HEAD.

## 2. The spec

The `tui` surface runs specs from `packages/tui/specs/` through the
PTY helper. One imperative spec that drives the real program:

```bash
cat > packages/tui/specs/docs-demo-hello.spec.mjs <<'EOF'
// Journey: greet — start the demo CLI, answer the prompt, expect the greeting.
import { spawnTui } from "../pty.mjs";

const cli = process.env.DEMO_CLI || "/tmp/probierz-demo-app/hello.mjs";
const tui = spawnTui("node", [cli]);
await tui.waitFor("What is your name?");
tui.send("Ada\r");
await tui.waitFor("Hello, Ada!");
const { code } = await tui.close();
if (code !== 0 && code !== null) {
  console.error(`demo CLI exited ${code}`);
  process.exit(1);
}
EOF
```

## 3. The manifest — the registration itself

```bash
mkdir -p apps/docs-demo
cat > apps/docs-demo/probierz.yaml <<'EOF'
schemaVersion: 1
appId: docs-demo
owner: docs
repositories:
  - root: /tmp/probierz-demo-app
    mappings:
      - paths: ["hello.mjs"]
        journeys: [greet]
surfaces:
  tui:
    spec: docs-demo-hello.spec.mjs
    journeys: [greet]
journeys:
  greet:
    owner: docs
    timeoutMs: 30000
pullRequestPolicy:
  minimumEvidence: E2
  requiredJourneys: [greet]
EOF
```

There is no register command; a valid manifest in place **is** the
registration ([concepts/application](concepts/application.md)).

## 4. Prove it registered

```
$ node agent/cli.mjs app docs-demo
{
  "schemaVersion": 1,
  "appId": "docs-demo",
  "owner": "docs",
  "repositories": [ { "root": "/tmp/probierz-demo-app",
      "mappings": [ { "paths": ["hello.mjs"], "journeys": ["greet"] } ] } ],
  "surfaces": { "tui": { "spec": "docs-demo-hello.spec.mjs", "journeys": ["greet"] } },
  "journeys": { "greet": { "owner": "docs", "timeoutMs": 30000 } },
  "pullRequestPolicy": { "minimumEvidence": "E2", "requiredJourneys": ["greet"] },
  "file": "<checkout>/apps/docs-demo/probierz.yaml"
}
```

Exit 0. The document you get back is the validated manifest plus its file
path — what every other command will read.

## 5. Validation is total, not advisory

Delete the `owner:` line and ask again:

```
$ node agent/cli.mjs app docs-demo
probierz-failure {"failure_point":"cli.unknown","error_code":"config","service":"cli","impact":"cli","severity":"critical","retryable":false,"outage":true,"detail":"invalid app manifest: <checkout>/apps/docs-demo/probierz.yaml owner is required"}
probierz app docs-demo: probierz is missing configuration. See the detail on the line above; retrying will not help.
```

Exit 1. An invalid manifest is an error, never a partial registration.
Restore the line before continuing.

## 6. The registry-wide view — blocked on this checkout, honestly

```
$ node agent/cli.mjs apps
probierz-failure {…"detail":"invalid app manifest: <checkout>/apps/game-asset-creator/probierz.yaml surface eval spec is required"}
probierz apps: probierz is missing configuration. See the detail on the line above; retrying will not help.
```

`apps` validates **every** registered manifest and fails closed, and this
revision ships one broken manifest — so the registry-wide commands (`apps`,
`status`, `overview`, `affected`, `ci`) are blocked until it is fixed. See
[limitations](limitations.md). Our registration is unaffected for every
single-app command, which is what the rest of this walkthrough uses.

## 7. Is the surface ready to run?

```
$ node agent/cli.mjs check tui
{
  "target": "tui",
  "ready": true,
  "checks": [
    { "name": "python3 pty.spawn shim", "ok": true, "own": false,
      "hint": "python3 stdlib provides the pty.spawn shim the TUI driver uses" },
    { "name": "node runtime", "ok": true, "own": false,
      "hint": "node is required for the TUI spec runner" }
  ],
  "missing": [], "remediation": []
}
```

Two honest edges of the discovery layer, captured:

```
$ node agent/cli.mjs describe packages/tui/specs/docs-demo-hello.spec.mjs
{ "spec": "packages/tui/specs/docs-demo-hello.spec.mjs", "count": 0, "outline": [] }

$ node agent/cli.mjs cmd tui
probierz-failure {…"detail":"unknown target: tui (one of web, electron, mobile:ios, mobile:android, desktop:mac, desktop:win)"}
```

`describe` outlines `describe`/`it` titles and sees nothing in an
imperative spec; `cmd` has no entry for `tui`
([limitations](limitations.md#cmd-covers-six-targets-run-covers-nine)).
Neither affects execution.

## 8. The identity a gate will demand

```
$ node agent/cli.mjs source-identity docs-demo
{ "harness": { "sha256": "74ab23def1e65b14ce964c1f7ccb3261f9ec4b43e2f081327115c113a3014331",
               "gitSha": "f8807e5d76ede5e5a797ec386119f87911fec2e5", "dirty": true, … },
  "app":     { "sha256": "bf76b35155387978d44936fb30019110c5791c32b26648d03c814e20935be6f2", … } }
```

`dirty: true` because this walkthrough's own uncommitted docs are part of
the harness worktree hash — identity binds to content, not to the git SHA
alone. These two values are exactly what `gate-evaluate` and `receipt`
take as expectations.

## Where to next

The application is registered. Producing evidence against it — runs, the
gate lifecycle, a signed receipt, the async queue — is
[walkthrough-gate-and-receipt](walkthrough-gate-and-receipt.md).
