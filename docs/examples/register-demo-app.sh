#!/bin/sh
# register-demo-app.sh — register a tiny demo application end-to-end:
# a real product repo, a TUI spec, and the manifest that IS the registration.
# Run from the probierz checkout root: sh docs/examples/register-demo-app.sh
# Walkthrough with captured output: docs/walkthrough-register-app.md
# Undo everything: sh docs/examples/cleanup-demo-app.sh
set -eu

[ -f agent/cli.mjs ] || { echo "run from the probierz checkout root" >&2; exit 2; }

# 1. The product: a one-prompt terminal program in its own git repository.
mkdir -p /tmp/probierz-demo-app
cat > /tmp/probierz-demo-app/hello.mjs <<'EOF'
#!/usr/bin/env node
// Minimal interactive TUI: asks for a name, greets, exits 0.
import readline from "node:readline";
const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
rl.question("What is your name? ", (name) => {
  process.stdout.write(`Hello, ${name || "stranger"}!\n`);
  rl.close();
});
EOF
if [ ! -d /tmp/probierz-demo-app/.git ]; then
  git -C /tmp/probierz-demo-app init -q
fi
git -C /tmp/probierz-demo-app add -A
git -C /tmp/probierz-demo-app -c user.email=docs@example.invalid -c user.name=docs \
  commit -qm "demo product" || true   # no-op when unchanged

# 2. The spec the tui surface will run.
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

# 3. The manifest — the registration itself. Validated in full on every load.
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

# 4. Prove it: the validated manifest back, the surface preflight, the identity.
node agent/cli.mjs app docs-demo
node agent/cli.mjs check tui
node agent/cli.mjs source-identity docs-demo
