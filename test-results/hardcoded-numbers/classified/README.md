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

Each number removed or moved at its source, and each finding still open, is recorded
in Oko against its product: the repair, its revision and its delivery state live there,
not in this repository, which holds the classification itself.

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
Neither complete reading nor a published source repair establishes an installed,
tested product repair.
