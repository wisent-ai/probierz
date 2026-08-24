# Pipeline: rules → evidence → compute

Where Probierz sits between its two neighbours: Tama owns write-time rules,
Probierz owns quality evidence and the release gate, Stado owns compute.
One flow, from commit to main:

```
commit → Tama hooks (write-time policy, separate repo)
→ push → probierz pre-push gate:
    affected journeys ← manifest file mappings
    → probierz ci (fresh preflight-gated runs, unless PROBIERZ_GATE_NO_CI=1)
    → newest passing run per affected journey
    → pull-request gate against the exact current source identity
→ green: push proceeds; red: blocked with the verdict's exact reasons
```

## What each neighbour owns

- **Tama** (separate repository) enforces repository rules at write time;
  Probierz neither installs nor evaluates those hooks. Its scanner is one of
  the external layers `probierz overview` reads and degrades gracefully
  without ([cli-evidence](cli-evidence.md)).
- **Probierz** — everything in this docs tree: manifests declare intent
  ([applications](applications.md)), runs produce bound evidence
  ([execution](execution.md), [evidence-model](evidence-model.md)), gates
  and signed receipts decide ([gates-and-receipts](gates-and-receipts.md)).
- **Stado** supplies remote capacity: `probierz stado run|author|seo`
  submits a job, evidence lands back in the local `test-results/` tree and
  is judged by the same gates ([remote-stado](remote-stado.md)).

## The developer-visible pieces

| Step | Command |
|---|---|
| install the gate into a product repo | `probierz gate-install <appId> --repo <path>` |
| what would the gate say right now | `probierz gate-prepush --repo <path> --app <appId>` |
| journey coverage and merge eligibility | `probierz status <appId> --text` |
| fresh evidence for a change | `probierz ci <base>` |
| sign release evidence | `probierz receipt <appId> <release> …` |

The installed hook chains any pre-existing `pre-push` hook, only gates
pushes that target `main`/`master`, and runs `probierz ci` first unless
`PROBIERZ_GATE_NO_CI=1` — the exact script it writes is in
[cli-gates](cli-gates.md#probierz-gate-install-appid---repo-path).

On this checkout the registry-walking steps are blocked by one invalid
registered manifest — see [limitations](limitations.md).
