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
