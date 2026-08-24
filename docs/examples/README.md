# Examples

Runnable scripts, each executed on this checkout before being committed.
All run from the probierz checkout root and need nothing beyond Node 22,
git, and openssl; none of them contacts a network or an external host.

| Script | What it proves | Walkthrough |
|---|---|---|
| `register-demo-app.sh` | a product repo + spec + manifest is a complete registration; prints the validated manifest, the tui preflight, and the exact source identity | [walkthrough-register-app](../walkthrough-register-app.md) |
| `run-gate-receipt.sh` | adhoc run refused by the gate (`run kind adhoc is not pull-request`), pull-request run evaluated green, gate activated and enforced, receipt signed at `--minimum E2` and verified untrusted vs trusted | [walkthrough-gate-and-receipt](../walkthrough-gate-and-receipt.md) |
| `mcp-async-run.mjs` | the in-process async queue over MCP stdio: start, poll to `passed`, list/read audited artifacts, cancel-as-no-op on a settled job | [mcp](../mcp.md#the-asynchronous-run-queue) |
| `cleanup-demo-app.sh` | deregistration is deletion: registration, spec, product, and demo evidence removed | — |

Order: `register-demo-app.sh` → `run-gate-receipt.sh` and/or
`mcp-async-run.mjs` → `cleanup-demo-app.sh`.

The demo lives outside version control on purpose: `apps/*` is gitignored
(registrations are local state) and the spec is created and removed by the
scripts, so the product's own `tui` suite never depends on the demo being
present.
