# MCP server

`probierz-mcp` (source entry point `agent/mcp.mjs`) exposes the same agent
modules the CLI uses as a stdio MCP server: newline-delimited JSON-RPC 2.0,
one response per request, `initialize` / `ping` / `tools/list` /
`tools/call`, with only protocol frames on stdout and diagnostics on stderr.
Tool descriptions preserve the read-only versus side-effecting boundary; a
client never receives implicit permission to install tooling, execute a
target, author a specification, or mutate a repository.

## Read-only tools

| Tool | Answers |
|---|---|
| `probierz_list_surfaces` | test surfaces: tool, npm script, targets, env |
| `probierz_list_specs` | spec files on disk, optional surface filter |
| `probierz_describe_spec` | static describe/it outline of one spec |
| `probierz_run_command` | the exact run command string (never executed) |
| `probierz_check` | preflight a target's toolchain, with fixes |
| `probierz_affected` | which targets a change could affect |
| `probierz_history` | E5 stability history: pass rate, flaky tests, trends |
| `probierz_dashboard` | product → version → journey evidence projection |
| `probierz_matrix_plan` | the declared nightly/release matrix, unexecuted |
| `probierz_source_identity` | exact harness and app source SHA-256 |
| `probierz_gate_status` | gate activation state |
| `probierz_status` | journey coverage, freshness, merge eligibility |
| `probierz_audit` | integrity-checked access audit records |
| `probierz_secret_scan` | high-confidence secret scan of a directory |
| `probierz_compare_runs` | deterministic diff between two runs |
| `probierz_last_green` | newest passing run |
| `probierz_verify_receipt` | receipt payload hash + Ed25519 verification |

## Side-effecting tools

- Execution: `probierz_setup`, `probierz_run`, `probierz_analyze`,
  `probierz_ci`, `probierz_run_matrix`, `probierz_gate_prepush`.
- Asynchronous runs: `probierz_start_run` returns a `runId` immediately;
  `probierz_run_status` polls queued/running/blocked/passed/failed/canceled;
  `probierz_cancel_run` terminates the complete process tree;
  `probierz_get_result`, `probierz_list_artifacts`, and
  `probierz_get_artifact` (bounded to 5 MiB per read, traversal rejected)
  read the outcome.
- Evidence: `probierz_protect_run`, `probierz_restore_bundle`,
  `probierz_retention`, `probierz_create_receipt`,
  `probierz_create_publication_manifest`.
- Gates: `probierz_gate_evaluate`, `probierz_gate_enforce`,
  `probierz_gate_activate`.
- Authoring: `probierz_author_spec`, `probierz_author_manifest`.
- Evaluators: `probierz_evaluate_figure`, `probierz_evaluate_seo`,
  `probierz_create_readme_gif`.
- Remote: `probierz_stado_run`, `probierz_stado_evaluate_seo`.

Each tool's exact argument schema is served by `tools/list`; the underlying
contracts are the same as the CLI's and are documented in
[execution](execution.md), [evidence-model](evidence-model.md),
[gates-and-receipts](gates-and-receipts.md), and
[evaluators](evaluators.md).
