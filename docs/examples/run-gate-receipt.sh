#!/bin/sh
# run-gate-receipt.sh — from a registered demo app to a verified signed receipt:
# adhoc run, kind refusal, pull-request run, gate activate/enforce, receipt,
# untrusted vs trusted verification. Requires register-demo-app.sh first.
# Run from the probierz checkout root: sh docs/examples/run-gate-receipt.sh
# Walkthrough with captured output: docs/walkthrough-gate-and-receipt.md
set -eu

[ -f apps/docs-demo/probierz.yaml ] || { echo "register first: sh docs/examples/register-demo-app.sh" >&2; exit 2; }

json() { node -e "const d=JSON.parse(require('fs').readFileSync(0,'utf8'));console.log(eval('d'+process.argv[1])??'')" "$1"; }
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# 1. Identities a gate demands — computed now, over current content.
node agent/cli.mjs source-identity docs-demo > "$WORK/id.json"
H=$(json .harness.sha256 < "$WORK/id.json")
S=$(json .app.sha256 < "$WORK/id.json")
echo "harness=$H"
echo "app=$S"

# 2. An adhoc run — passes, but can never satisfy a pull-request gate.
node agent/cli.mjs run tui --app docs-demo > "$WORK/adhoc.json"
ADHOC=$(json .runId < "$WORK/adhoc.json")
echo "adhoc run: $ADHOC passed=$(json .passed < "$WORK/adhoc.json")"
node agent/cli.mjs gate-evaluate docs-demo pull-request "$H" --source-sha "$S" --runs "$ADHOC" \
  > "$WORK/eval-adhoc.json" && rc=0 || rc=$?
echo "gate-evaluate (adhoc) exit=$rc verdict=$(json '.verdict.errors[0]' < "$WORK/eval-adhoc.json")"

# 3. A pull-request run: the kind travels as a recorded run condition.
node agent/cli.mjs run tui --app docs-demo PROBIERZ_RUN_KIND=pull-request > "$WORK/pr.json"
PR=$(json .runId < "$WORK/pr.json")
echo "pull-request run: $PR passed=$(json .passed < "$WORK/pr.json")"

# 4. Green evaluation, activation, enforcement.
node agent/cli.mjs gate-evaluate docs-demo pull-request "$H" --source-sha "$S" --runs "$PR" > "$WORK/eval-pr.json"
echo "gate-evaluate (pull-request) passed=$(json .verdict.passed < "$WORK/eval-pr.json")"
node agent/cli.mjs gate-activate docs-demo pull-request "$H" --source-sha "$S" --runs "$PR" > "$WORK/activate.json"
echo "gate enforcement now: $(json '.config.modes["pull-request"].enforcement' < "$WORK/activate.json")"
node agent/cli.mjs gate-enforce docs-demo pull-request "$H" --source-sha "$S" --runs "$PR" > "$WORK/enforce.json"
echo "gate-enforce passed=$(json .verdict.passed < "$WORK/enforce.json")"

# 5. Sign a receipt at the level the evidence actually has (E2: no recording),
#    then verify — untrusted with the embedded key, trusted by fingerprint.
openssl genpkey -algorithm ed25519 -out "$WORK/receipt-key.pem"
export PROBIERZ_RECEIPT_PRIVATE_KEY_FILE="$WORK/receipt-key.pem"
node agent/cli.mjs receipt docs-demo v0.2.0-docs "$H" --source-sha "$S" --runs "$PR" --minimum E2 \
  > "$WORK/receipt.json"
RFILE=$(json .file < "$WORK/receipt.json")
FP=$(json .receipt.signing.publicKeyFingerprintSha256 < "$WORK/receipt.json")
echo "receipt: $RFILE"
node agent/cli.mjs verify-receipt "$RFILE" > "$WORK/verify-untrusted.json" && rc=0 || rc=$?
echo "verify (no trust anchor) exit=$rc valid=$(json .valid < "$WORK/verify-untrusted.json") signatureValid=$(json .signatureValid < "$WORK/verify-untrusted.json")"
node agent/cli.mjs verify-receipt "$RFILE" --fingerprint "$FP" > "$WORK/verify-trusted.json"
echo "verify (pinned fingerprint) valid=$(json .valid < "$WORK/verify-trusted.json")"
