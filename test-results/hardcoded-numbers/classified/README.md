# Numeric literals assigned in Wisent product code, read line by line

Source list: `../numeric-literals-assigned.txt` (19 577 lines outside `archive` and `oh-my-pi`).
Product folders hold tab-separated source paths, source line numbers, classes and notes.
Continuation files use `numbers-02.tsv`. Files under `assigned/` and product `assigned.tsv`
files refer to inclusive line ranges in the original assignment list; every line in each
recorded range was read. `assigned/source.json` identifies the exact input by SHA256 and
records the contiguous reviewed ranges separately from excluded ranges.
These are partial review records, not proof that the entire assignment list has been covered.

## Classes

- `COUNTER`: a loop index, accumulator, generation or flag that starts at a value. Not a setting.
- `PROTOCOL`: fixed by a protocol, file format, algorithm, OS constant, exit-code contract or another product's declared bound.
- `SCHEMA`: a document or wire schema version, structural field count or workflow node identity.
- `LAYOUT`: point sizes, geometry, padding and displayed precision.
- `GEOMETRY`: coordinates of a modelled 3D shape (artwork data).
- `NOT`: the match is a comment, help text or code inside a string.
- `TUNING`: a chosen limit, interval, size, retry count or default. These are the numbers somebody picked.
- `PRICE`: a vendor price written in code.
- `REMOVED`: a number that had no basis and was deleted at its source.
- `EXPERIMENT`, `HYPER`, `GAME`: experiment inputs, model settings or game definitions.
  These labels describe their role; they do not prove that an embedded value is justified.
- `VENDOR`: downloaded third-party implementation. Review does not authorize editing it.

## Removed or moved at the source

| Product | Number | Commit |
|---|---|---|
| las | `maximumEntries = 10_000` cut directory counts at 10 000 | 33c6982 |
| weles | `MAX_EVIDENCE_FILES = 10_000` refused evidence trees | 848c8be8 |
| weles | Oxylabs `GB_PRICE = 9` | 3adc46c6 |
| weles | Pangram `walkJson` stopped at 20 000 result files | ada966c5 |
| jeden | search walk stopped at `MAX_SEARCH_FILES = 20_000` | 305bef4 |
| brama | `LOG_TAIL = 60` lines of the unit log; diagnosis reads the boot attempt from the launcher's first line | e14e614b |
| brama | `LARGE_MODEL_VRAM_GB = 24`, `SMALL_MODEL_VRAM_GB = 8` and the model recommendation they chose | e14e614b |
| brama | `MAX_KEY_BYTES = 4096`; the bound is the longest valid key | e14e614b |
| brama | `ERROR_PREVIEW_CHARS = 512`, `EVIDENCE_CHARACTERS = 200`, `RESPONSE_EXCERPT_CHARS = 300`, task-quality output cut at 1500 | e14e614b |
| brama | `MAX_QUALITY_MODELS = 25`; the operator's `--max-models` is the count | e14e614b |
| brama | `MAX_MODEL_ID_BYTES = 512`, `MAX_PROVIDER_ID_BYTES = 128` (two copies), `MAX_PATH_SEGMENT_BYTES = 128` | e14e614b |
| brama | `DEFAULT_TTL_SECONDS = 900`; `BRAMA_MODEL_CATALOG_TTL_SECONDS` or the process lifetime | e14e614b |
| brama | `DEFAULT_WORKLOAD_UID/GID = 10001`; the caller's account | e14e614b |
| brama | refresh margin, plan-usage jitter, HMAC window, grant lifetime/uses/rate: need the operator's value | Oko defect 39fa606b |
| weles | `MAX_EDGE_TEXT = 240`, `MAX_WITHHELD_EDGES = 2048` (two copies), `MAX_EDGE_LINE_BYTES = 4096`, 2 MiB ledger cap | fcbffbec |
| weles | MCP `EVENT_LIMIT = 500`; events are kept until read | fcbffbec |
| weles | `MAX_PAGE_DETAIL_CHARS = 400`, `DEFAULT_LIST_LIMIT = 20` | fcbffbec |
| weles | burned-proxy and capability `CACHE_TTL_MS = 60000` | fcbffbec |
| weles | `STDOUT_RING_CAP = 50000`; capture is per live session | fcbffbec |
| weles | `VISION_MAX_OUTPUT_TOKENS = 4096` | fcbffbec |
| weles | `QUOTED_BODY_CHARS = 300`, action and account-id length quantifiers | fcbffbec |
| weles | `RUN_OUTPUT_DEPTH = 3`, `RUN_OUTPUT_MAX_BYTES` | fcbffbec |
| weles | `CONTROL_LIMIT = 4096` is Skarbiec's broker control-line bound (PROTOCOL, as in brama) | — |
| weles | admission, page-route, records-route, signup, Apple-expiry, vision-geometry, proxy-rate and trajectory poll values | Oko defect 30817d3d |
| stado | janitor `MAX_DEPTH = 64`, `MAX_WORKDIR_DEPTH = 256`, scratch `MAX_DEPTH = 256` | 3bfd3e1b |
| stado | janitor `MAX_ERRORS = 16` and the 128-character error filter | 3bfd3e1b |
| stado | `SERVICE_LOG_MAX_BYTES`, `SERVICE_LOG_KEEP_BYTES`, `SERVICE_LOG_SCAN_LIMIT = 512`; logs are emptied only at the disk-full threshold | 3bfd3e1b |
| stado | billing watch `MESSAGES_READ = 500`; every message in the window is read | b4552ef6 |
| stado | `build list --limit` default 20, `SUPERSEDED_SCAN = 20`, `SCAN_WINDOW = 120` | b4552ef6 |
| stado | `machine logs --limit` default 65536 and the watch's page constant | b4552ef6 |
| stado | `EXCERPT_CHARS = 40`, `BODY_EXCERPT_BYTES = 200`, `ERROR_PREVIEW_MAX = 1024`, `FAILURE_FIX_PROMPT_ERROR_BYTES = 4000` and the fixer's 600/500/300/160 cuts | 1d2dce91 |
| stado | `UNCOVERED_ROWS = 12`, release-cause `EVIDENCE_CHARS = 240` | 1d2dce91 |
| stado | `NEWEST_SILENCES = 5`; every silence is listed, and repair reads the one that can be open | 66a01415 |
| stado | scan windows, per-tick caps, intervals, TTLs, ports, grant lifetimes, display windows, the 80 % threshold | Oko defect fab304dc |
| tama | `FOLDER_FILE_LIMIT = 5`, `FILE_LINE_LIMIT = 300` restated in code; read from `numeric-provenance.json` | 4879e2c9 |
| tama | `GH_LIST_LIMIT = 1000`; every repository through paginated GraphQL | 4879e2c9 |
| tama | hook and command thresholds not in `numeric-provenance.json` | Oko defect ee36eefc |
| oko | transcript listing `FIRST_LINE = 140`; the terminal's width, whole when not a terminal | 508292c8 |
| oko | autonomy, calendar, judge, goal, index, telemetry, suggestion and relay values | Oko defect ed9034ca |
| jeden | `LOCAL_OUTPUT_MAX_LINES = 200`, `REFUSAL_EXCERPT_CHARS = 120` | 0c0287ac |
| jeden | loop, recovery, route, capability, billing, port, buffer and budget values | Oko defect ee1df8fa |

## The 10 000-file cleanup cap

The per-host `disk_cleanup.max_items_per_pass` setting and its validator were removed
with the retirement of per-host janitor declarations. The published source now follows
the built-in disk-full rule without item, byte or scan caps. Source publication does not
establish which release a host is running. Origin, publication and deployment evidence
are recorded in Oko.

## Read so far

The product-specific records include stado, weles, tama, oko, brama, jeden, most, lem,
glina, skarbiec, skrzynka, probierz, spis, zwiad, ster, potyczka, echo-web and wisent-app.
The corpus-bound range records additionally cover the opening repositories, backend
inputs and subsequent desktop/service entries. The range ledger is not a completion
claim for all files in those repositories. Findings still require source remediation
or a concrete blocker recorded in Oko.
