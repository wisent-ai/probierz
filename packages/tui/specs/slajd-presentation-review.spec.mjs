// Journey presentation-review: a speaker has slajd review judge a
// presentation against the document it presents: one Slajd rendered with its
// speaker script, and the same slides as an external PDF given without one.
// The review answers with every rubric criterion judged and the report holds
// the SHA-256 of every slide and the source; a presentation that misstates its
// source is not passed and fails source fidelity.
//
// Declarations the journey reads, each refused by name when missing:
// TUI_CMD, the slajd executable under test; SLAJD_TOKENS, the
// wisent-components tokens.json; SLAJD_SLIDE_WIDTH, the slide PNG width;
// SLAJD_REVIEW_MODEL, the vision model that judges. slajd reaches the model
// router through Stado's capability resolution and finds probierz on PATH or
// in SLAJD_PROBIERZ_BIN.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

function declared(name) {
  const value = process.env[name];
  if (!value) {
    throw new Error(`${name} is not set; the presentation-review journey needs it`);
  }
  return value;
}

function fail(message) {
  throw new Error(`presentation-review: ${message}`);
}

const slajd = declared("TUI_CMD");
const tokens = declared("SLAJD_TOKENS");
const width = declared("SLAJD_SLIDE_WIDTH");
const model = declared("SLAJD_REVIEW_MODEL");

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "test-results", `slajd-review-${process.pid}`);
rmSync(root, { recursive: true, force: true });
mkdirSync(root, { recursive: true });

const source = [
  "# Grant application",
  "",
  "The project costs 1.2 million euro and runs for 24 months.",
  "It trains one open language model and publishes its weights.",
].join("\n");
const talk = (cost) =>
  [
    "---",
    "title: Grant talk",
    "footer: Example Lab",
    "duration: 2",
    "---",
    "",
    "<!-- layout: cover -->",
    "<!-- minutes: 1 -->",
    "",
    "# Grant talk",
    "",
    "## One open model in two years",
    "",
    "---",
    "",
    "<!-- layout: bullets -->",
    "<!-- minutes: 1 -->",
    "",
    "# The project",
    "",
    `- **Cost** ${cost} million euro over 24 months`,
    "- **Result** one open language model with published weights",
    "",
    "<!--",
    `We ask for ${cost} million euro for two years, and the weights are published.`,
    "-->",
  ].join("\n");

writeFileSync(join(root, "application.md"), source);

function run(args) {
  const answer = spawnSync(slajd, args, { cwd: root, encoding: "utf8" });
  if (answer.error) fail(`slajd could not run: ${answer.error.message}`);
  return answer;
}

function render(name, cost) {
  const directory = join(root, name);
  mkdirSync(directory, { recursive: true });
  writeFileSync(join(directory, "talk.md"), talk(cost));
  const rendered = run([
    "render",
    "--talk", join(directory, "talk.md"),
    "--output", join(directory, "talk.pdf"),
    "--tokens", tokens,
    "--slides", join(directory, "slides"),
    "--slide-width", width,
    "--notes", join(directory, "script.md"),
  ]);
  if (rendered.status) fail(`slajd render exited ${rendered.status}: ${rendered.stderr}`);
  return directory;
}

function review(name, args, slides) {
  const output = join(root, `${name}.json`);
  const answer = run(["review", ...args, "--source", join(root, "application.md"), "--model", model, "--output", output]);
  if (answer.status !== 0 && answer.status !== 2) fail(`${name}: slajd review exited ${answer.status}: ${answer.stderr}`);
  const verdict = JSON.parse(answer.stdout);
  if ((answer.status === 0) !== verdict.pass) fail(`${name}: exit ${answer.status} does not match pass ${verdict.pass}`);
  const report = JSON.parse(readFileSync(output, "utf8"));
  if (report.inputs.slides.length !== slides) {
    fail(`${name}: the report names ${report.inputs.slides.length} slides for ${slides}`);
  }
  const digest = createHash("sha256").update(readFileSync(join(root, "application.md"))).digest("hex");
  if (report.inputs.sources[0].sha256 !== digest) fail(`${name}: the report's source digest is not the source's`);
  for (const criterion of verdict.criteria) {
    const judged = report.evaluation.dimensions[criterion.name];
    const evidenced = Array.isArray(judged.evidence) && judged.evidence.some((line) => line.trim());
    if (typeof judged.met !== "boolean" || !evidenced) fail(`${name}: ${criterion.name} is not judged with evidence`);
  }
  return verdict;
}

const faithful = render("faithful", "1.2");
const misstated = render("misstated", "3.5");
const slides = readdirSync(join(faithful, "slides")).filter((file) => file.endsWith(".png")).length;

const own = review("own", ["--presentation", join(faithful, "slides"), "--script", join(faithful, "script.md")], slides);
const wrong = review(
  "misstated",
  ["--presentation", join(misstated, "slides"), "--script", join(misstated, "script.md")],
  slides,
);
if (wrong.pass) fail("a presentation that misstates its source's cost was passed");
if (wrong.criteria.find((criterion) => criterion.name === "source_fidelity")?.met) {
  fail("the misstated cost was judged faithful to the source");
}

// The same misstated slides as an external PDF, given without a speaker script.
copyFileSync(join(misstated, "talk.pdf"), join(root, "external.pdf"));
const external = review("external", ["--presentation", join(root, "external.pdf")], slides);
if (external.pass) fail("an external PDF that misstates its source's cost was passed");

rmSync(root, { recursive: true, force: true });
console.log(
  `presentation-review: own talk ${own.pass ? "passed" : "was refused"}; the misstated talk and the external PDF were refused`,
);
