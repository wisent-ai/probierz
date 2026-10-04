# Numeric literals assigned in Wisent product code, read line by line

Source list: `../numeric-literals-assigned.txt` (19 577 lines outside `archive` and `oh-my-pi`).
Product folders hold tab-separated source paths, source line numbers, classes and notes.
Continuation files use `numbers-02.tsv`. Files under `assigned/` and product `assigned.tsv`
files refer to inclusive line ranges in the original assignment list; every line in each
recorded range was read. `assigned/source.json` identifies the exact input by SHA256 and
records the contiguous reviewed ranges separately from excluded ranges.
The range ledger covers the complete assigned corpus outside the two excluded directories.
Reading coverage is separate from repair, publication and real-product verification.

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
| most | waits, backoffs, heartbeats, helper and relay ports, length bounds | Oko defect 70151637 |
| lem | detector output `clippedOutput(limit: 8_000)`; an unknown verdict decided by `>= 50` | 0e9031d7 |
| lem | Oko agenda failure body `data.prefix(500)` | 6f291402 |
| lem | model budgets, harvest bounds, generation defaults, backend port | Oko defect 68645422 |
| glina | Glina Desktop `tail(maxCharacters: 400)` | glina-desktop a5795125 |
| glina | preview, render, tessellation and generation values | Oko defect 0df1bf55 |
| skarbiec | Skarbiec Desktop `previewLimit = 128_000`, `detailExcerptLength = 320` | skarbiec-desktop 8ed9664b |
| skarbiec | lifetimes, input bounds, concurrency, import cap, Desktop waits and windows, Hub bounds | Oko defect cd6f092c |
| skrzynka | `message list`/`outbound` `--limit` default 100 and the silent clamp to 1-500 (CLI and API) | a5981f8b |
| skrzynka | Skrzynka Desktop `defaultListLimit = 300`, `maxListLimit = 500`, `responseCeiling` | skrzynka-desktop 3f4c6361 |
| skrzynka | poll, OAuth, sync, send bounds, token margin, Desktop wait | Oko defect 9d9bef81 |
| ster | Brama refusal `BODY_EXCERPT = 240` | b8cedf58 |
| ster | training, synthesis, calibration and quality defaults | Oko defect 20f71342 |
| landings | `TITLE_LENGTH_LIMIT = 65` in landing-cli's template and 14 generated landings | landing-cli d77ee59b |
| echo-web | windows, list caps, generation defaults, money tolerances, alert floors | Oko defect 9c257cee |
| las | budgets, ranking, waits, credential policy, display bounds | Oko defect 0baf3362 |
| skryba | generation, training and quality defaults | Oko defect 629666b4 |
| preferences | ranking thresholds, priors, vote weights, import caps | Oko defect 7dc96443 |
| probierz | figure `ERROR_EXCERPT = 500`, `RENDER_FAILURE_EXCERPT = 4_000`, `benchmark list --limit` default 20 | 060be337 |
| probierz | grading budgets, router wait, source bounds, benchmark and Desktop values | Oko defect 344dc59b |
| brama | Brama Desktop `maximumErrorExcerptBytes = 4096`, `listedModels = 8`, `maximumIdentifierCharacters = 128` | brama-desktop f3fc3f6e |
| brama | Brama Desktop thresholds, history, Stado wait, readiness bounds | Oko defect d8cafe10 |
| needher-ai-web | credits, feed caps, ranking and bandit policy, render values | Oko defect 99fbac2e |
| growth-tactics | prices in code, cost recorded as zero, waits, media and result values | Oko defect 62ba6752 |
| potyczka | projectile pool, rendering detail and animation timing | Oko defect 140a58b6 |
| wisent-app | web analytics `MAX_QUEUE_SIZE = 100` | 09bbc1b5 |
| wisent-app | IBKR and Tavily failure bodies cut to 300 and 400 | 3316156e |
| wisent-app | generation defaults, silent clamps, waits, thresholds, server port | Oko defect 2f95d07c |
| OpenEnv | benchmark rounds, thresholds, fallbacks, training settings, server port | Oko defect dcbdb704 |
| backends | scratch and research scripts outside any product | Oko defect 51273293 |
| brama | Brama Desktop readiness `maximumEntries = 256`, `maximumSentenceCharacters = 512`, `stadoCallTimeoutSeconds = 120`, `bundleAncestors = 14`, `ancestorWalk = 10` | brama-desktop d74fa5bc |
| stado | `release status` `RUN_WINDOW = 10`, web deploy `RUN_WINDOW = 32` | 8ff2b125 |
| transcript-lake | adapter and stream `TEXT_CAP = 65536`, `EXTRA_DEPTH = 4`, `PENDING_CAP = 64`, read `substr(text, 1, 240)`, `DEFAULT_LIMIT = 20`, `MAX_LIMIT = 500`, `DEFAULT_DAYS = 7`, `SHOW_LIMIT = 2000`, `SHOW_MAX_LIMIT = 50000` | 9053e2ea; docs transcript-lake-landing f54073a8 |
| transcript-lake | goal artifact size, title bounds, redaction thresholds, export buffers, batch | Oko defect f6cd0556 |
| transcript-label-trainer | `ERROR_EXCERPT = 160` and error/answer cuts of 80-300 characters | 3c3004f0 |
| transcript-label-trainer | answer budgets, discovery sampling, concurrency, training and evaluation settings | Oko defect 9e410b04 |
| trading-autonomy | comment `maxDepth = 4`, proxy `maxMessageLength = 10000` and last 20 messages, feed `MAX_ENTRIES_PER_AGENT = 3` | bb6b65da |
| trading-autonomy | cadences, balances, slippage, spend ceilings, windows, priors, estimates, ports | Oko defect 3bd83977 |
| wisent-backend | `_MAX_CAUSE = 300`, `_DETAIL_CHARACTERS = 400` | a86adb51 |
| wisent-backend | inference, notification, relationship and messaging policy, waits, previews, zero fallbacks | Oko defect 47a53330 |
| wisent-backend-mlx-local-fork | ports, inference defaults, model ids, waits, zero fallbacks | Oko defect cdc747b5 |
| wisent-ios | `maxDiagnosticLength = 300` | a051d854 |
| wisent-ios, turbot-ios, oko-ios | analytics `maxQueuedPayloads = 200` (defect d6df8d4d recorded and repaired for oko-ios) | wisent-ios 92f8a524; turbot-ios 8acebec0; oko-ios c883f44e |
| wisent-ios | speech log excerpts `text.prefix(50)` (six lines now log the character count) | 80e7ec52 |
| wisent-ios | tokenizer ids, bounds, waits, progress steps, speech and diffusion settings | Oko defect 031b301b |
| wisent-android | quotas, page sizes, waits, field bounds | Oko defect 446e06e4 |
| wisent-core | intervention, classifier and guard settings, model-name shape guesses | Oko defect e28d4138 |
| wisent-evaluators | thresholds, weights, waits, zero confidence fallbacks | Oko defect 219e8f3f |
| wisent-integrations | provider bounds, spend ceilings, revenue zero defaults, JWT and RSA policy | Oko defect 84eeca16 |
| wisent-components | Figma `DESCRIBED_PER_PAGE = 30`, `LISTED_PER_PAGE = 120`, `STYLES_PER_PAGE = 40`, `COMPONENTS_PER_PAGE = 8` (pages measured by what the tool's answer carries); docs search 12 hits and 180 characters, error excerpts 300/400/1,500, three failures, eight shadows; defect 9387e5ef repaired | cee94bc2; 170625b4 |
| wisent-enterprise | `JsonDetails max = 800` (and 1,500/2,000/5,000 at call sites), `sanitizeText max = 220`/300, array `slice(0, 4)`, capture `fileLimit = 50` | 80d5c5a9 |
| wisent-enterprise | verdict windows, import caps, polling, page size | Oko defect 11bd16d4 |
| wisent-experiments | zero-average fallbacks, fixed BOS id | Oko defect 5c8192dd |
| wisent-body-horror | rule gates and weights, zero fallbacks, person count | Oko defect 5f1f4de4 |
| stado | space report and janitor ignored the volume of a declared `work_root` | 0c1d87cd; docs stado-landing b8948e7d (defect 08c47436) |
| wisent-errors | `DETAIL_LIMIT = 2000` / `detailLimit` (all four languages), default trim width and `slack = 24`, Swift reporter `timeoutInterval = 5`, scanner `THRESHOLD = 4` (now a majority of the catalogue), `SHORT_PIN_LENGTH = 8` | 1afc2538 |
| wisent-customer-support | `MAX_DETAIL_CHARS = 400` | a2ca8f13 |
| wisent-customer-support | low-confidence threshold | Oko defect a345adb8 |
| wisent-desktop-auth | `maxDiagnosticLength = 400` | eb2d4318 |
| wisent-desktop-auth | identity wait, resend cooldown | Oko defect 0db57b4a |
| wisent-gradio | `_MAX_DETAIL_CHARS = 500`, `SHOWN_PAIRS = 6` | 8e6dc866 |
| wisent-gradio | onboarding bounds, copied chunk size, waits, workers, split | Oko defect 7f4d7b3a |
| wisent-trade | failure `MAX_DETAIL_CHARS = 300` | 5d6a1e75 |
| wisent-trade | analytics `MAX_QUEUE_SIZE = 100`, `getRecentTrades` default limit 50 | 5289d702 |
| wisent-trade | analytics bounds, queue size, recent trades, polling, price | Oko defect 30384295 |
| wisent-landing | failure `MAX_DETAIL_CHARS = 300`, unused `visibleItemsCount = 3` | local 206166f, not pushed: repository archived on GitHub |
| wisent-landing-new | blog page size; archived wisent-landing waits, zones, HSTS | Oko defect 326fb734 |
| wisent-supabase-oko | `SUPERSEDED_SUBSCRIPTION_LIMIT = 100` (Stripe list now paged to the end) | bab7687b |
| wisent-supabase-oko | push alert cut, seats, checkout, model settings, email bounds | Oko defect d181d72b |
| wisent-tour-bridge | `SKIPPED_TEXT_HEAD = 60` (Python; edit refused), waits, skew | Oko defect 93b7a6c9 |
| wisent-extractors | dataset waits, item cap, guessed answers, zero fallbacks | Oko defect dc9f2900 |
| wisent-optimizer | sentinels, split, bounds, zero results | Oko defect 8ed86ace |
| wisent-terminal-session-recovery | relaunch and cadence policy | Oko defect e38ba4df |
| wisent-marketing-asset-generator | Brama refusal `snippet` cut at 157 characters | 3c9f65a3 |
| wisent-logo-generator, wisent-marketing-asset-generator | handcrafted quality scores, thresholds, input limits | Oko defect 464366bb |
| wisent-uncensored-model | training reward settings | Oko defect 1686ea15 |
| wisent-model | generation bounds and weights | Oko defect 9bad74b1 |
| wisent-ios-repo | credits, counts, waits, zero score | Oko defect 9f9c1732 |
| wisent-visuals | zero in median, cache, encoding, wait | Oko defect ba52a0a1 |
| wisent-cost-tracker | sink wait, rounding precision | Oko defect 38bd28c5 |
| wisent-supabase-wisent-app | sync batch bound | Oko defect 9a3f7ffd |
| wisent-node | inference scale default | Oko defect d2f793af |
| wisent-ground-truth-api | extractive source count | Oko defect d0f380ac |
| singularity | `MAX_ERROR_EXCERPT_CHARS = 800` (Brama and Most), `MAX_AGENT_ID_BYTES = 128` | 5d67aee9 |
| singularity-desktop | `shownActivityLimit = 250`, `retainedActivityLines = 2_000` | c3e083ea |
| singularity | bounds, lifetimes, retention, page size, import cap; Desktop recent rows | Oko defect 002f391e |
| trends | `observations` `DEFAULT_LIST_LIMIT = 50` (optional `--limit`), `SNIPPET_CHARS = 200` | 56fd7342 |
| trends | ingest per-source cap, fetch bounds, detection settings | Oko defect 890d3f0b |
| quality-control | `SOURCE_EXCERPT_LIMIT = 160`, `DETAIL_EXCERPT_LIMIT = 32` | f75b8422 |
| quality-control | gate windows, duplicated file limit | Oko defect 4cf2b319 |
| pursuit, product-guidelines | preference limit, rounds, attempts, sentence minimum | Oko defect 777698aa |
| rachuba | tax rules in code | Oko defect 627c1ef9 |
| research | quality targets, delays, inference settings | Oko defect a4d638c8 |
| trading-tools | body limit, waits, cache age | Oko defect 85948b57 |
| turbot-ios, turbot-web | waits, credits, counts, queue, archive bounds | Oko defect 135d0fa7 |
| ugc-cli | portal validity, discovery, matching | Oko defect 0c7ff063 |
| uncensorbench | ports, scoring, sampling | Oko defect 53a5a13b |
| wisent-1b, wisent-agent | model settings, zero metrics | Oko defect 74c127fb |
| wisent-backend-images | detail cut (Python, edit refused), waits, generation bounds, port | Oko defect 8010b87f |
| wisent-backend-test-day | bounds, lifetimes, cadence, token length | Oko defect e13e5b66 |
| competitor-research | `ERROR_EXCERPT_LENGTH = 300` | 6479ddf6 |
| people-rotator | `ERROR_DETAIL_LIMIT = 300` | 7acfd5ae |
| people-rotator, patent-cli | id length, patent guidelines and deadlines | Oko defect d212ee6a |
| compute.wisent.com | failure `MAX_DETAIL_CHARS = 300` | 67ec8906 |
| compute.wisent.com | web analytics `MAX_QUEUE_SIZE = 100` | 4e40f1c2 |
| compute.wisent.com | prices, hardware, ports, security parameters, cadences | Oko defect 5a051a49 |
| codespy | `SHOWN_LINE_CHARS = 80` | a39e9439 |
| codespy | removed the file-size ceiling and built-in scoring weights/grades/size leniency; optional explicit `--scoring-policy`, nullable score without policy, full policy in every report and Action; CLI regression journeys added but not run | 9cc178a (source version 3.0.0); Oko fa8f1c10 records pushed repair, open until real qualification and release |
| people-rotator | `sanitizeId(value, maxLength = 48)` and idempotency keys cut to 128 | bddce0e9 |
| people-rotator | integration failure body cut to 300 | 24ce2cb4 |
| echo | `DEFAULT_LIMIT = 200`, `MAX_LOGS = 5000`, `MAX_ERRORS = 10`, `MAX_CHAT_EVENTS = 5000` (every read now pages to the exact count) | fbabc0b0; docs echo-landing f9741bed |
| echo-desktop | `maximumEntries = 10_000`, `defaultLimit = 200`, `maxLogs`, `maxErrorsPerKey`, `maxChatEvents`, `errorBodyPrefix = 500` | aa77397f |
| echo, echo-web, echo-desktop | windows, onboarding caps, durations, tolerance, volumes | Oko defect dc2e1d6e |
| byk-desktop | `oneLine(limit: 90)` | c68c43aa |
| byk-ios, byk-desktop | analytics `maxQueuedPayloads = 200`, error bodies cut to 200 and 160 | byk-ios b5e6e508; byk-desktop 5ea7c602 |
| byk-desktop, byk-ios | waits, queue, revision bound, code length | Oko defect b0d3f955 |
| deep-analytics | `clean(value, maxLength)` cuts of 24-1,000 characters at 71 call sites, `MAX_DECODED_HTML_CHARS`, `MAX_KEYWORD_CHARS = 180`, `MAX_REGION_CHARS = 24`, error excerpts 300/500 | a082f568 |
| deep-analytics, competitor-research | bandit and graduation policy, text cuts, page sizes | Oko defect 3f41319f |
| handtohuman | refused payment error cut to 400 characters | 6ffa39aa |
| handtohuman | prices, minimums, payment and egress limits | Oko defect e8708637 |
| creator-portal | `RECENT_SUBMISSIONS_LIMIT = 5` | 544fd90c |
| creator-portal | verification `CODE_LENGTH = 6` (one field takes the auth service's code at its own length); defect 18fb6ae7 repaired | 87c8ef50 |
| film, grant-cli | default frames, rate, dimensions and HD tier (film), 8-character attachment confirmation (grant-cli), by session 01a10407 | film d6d51e2e; grant-cli f35a9233 |
| cntrlai, iskra | training and evaluation settings (Python; edit refused; iskra deletion is another session's task) | Oko defect eec9b5b4 |
| zwiad | fixed winners and optimal band in figures, invented curves, thresholds, zero fallbacks, `TABLE_ROWS = 10` (Python; edit refused) | Oko defect 30d9d67e |

## The 10 000-file cleanup cap

The per-host `disk_cleanup.max_items_per_pass` setting and its validator were removed
with the retirement of per-host janitor declarations. The published source now follows
the built-in disk-full rule without item, byte or scan caps. Source publication does not
establish which release a host is running. Origin, publication and deployment evidence
are recorded in Oko.

## Reading coverage

The reviewed original-list ranges are 1–785, 32688–37213 and 54381–68646:
19 577 records in total. The archive and oh-my-pi ranges remain excluded.
The source digest, exclusions and absence of deferred or partial reads are recorded
in `assigned/source.json`. Large bundled records were read through their complete
source where a clipped preview could not establish coverage.

This is a claim about the assigned snapshot, not every file in every repository.
The table above distinguishes published source repairs from unresolved findings in Oko.
Neither complete reading nor a published source repair establishes an installed,
tested product repair.
