// Journey talk-review: a speaker renders a talk with the lem-cli under test and
// has Probierz judge it against the document it presents. The review answers
// with every rubric criterion judged, the evidence for each, and the SHA-256
// of every slide, the script and the source; a talk that misstates its source
// is not passed.
//
// Declarations the journey reads, each refused by name when missing:
// TUI_CMD, the lem-cli executable under test; PROBIERZ_BIN, the probierz
// executable that judges; LEM_WISENT_TOKENS, the wisent-components tokens.json;
// LEM_TALK_SLIDE_WIDTH, the slide PNG width; PROBIERZ_TALK_MODEL,
// STADO_MODEL_ROUTER_URL and STADO_MODEL_ROUTER_TOKEN, the judge and its route.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

function declared(name) {
  const value = process.env[name];
  if (!value) {
    throw new Error(`${name} is not set; the talk-review journey needs it`);
  }
  return value;
}

function fail(message) {
  throw new Error(`talk-review: ${message}`);
}

const lem = declared("TUI_CMD");
const probierz = declared("PROBIERZ_BIN");
const tokens = declared("LEM_WISENT_TOKENS");
const width = declared("LEM_TALK_SLIDE_WIDTH");
for (const name of ["PROBIERZ_TALK_MODEL", "STADO_MODEL_ROUTER_URL", "STADO_MODEL_ROUTER_TOKEN"]) {
  declared(name);
}

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "test-results", `lem-talk-review-${process.pid}`);
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

function run(program, args) {
  const answer = spawnSync(program, args, { cwd: root, encoding: "utf8" });
  if (answer.error) fail(`${program} could not run: ${answer.error.message}`);
  return answer;
}

function review(name, cost) {
  const directory = join(root, name);
  mkdirSync(directory, { recursive: true });
  writeFileSync(join(directory, "talk.md"), talk(cost));
  const rendered = run(lem, [
    "talk", "render",
    "--talk", join(directory, "talk.md"),
    "--output", join(directory, "talk.pdf"),
    "--tokens", tokens,
    "--slides", join(directory, "slides"),
    "--slide-width", width,
    "--notes", join(directory, "script.md"),
  ]);
  if (rendered.status) fail(`lem-cli talk render exited ${rendered.status}: ${rendered.stderr}`);
  const report = join(directory, "review.json");
  run(probierz, [
    "evaluate", "talk",
    "--slides", join(directory, "slides"),
    "--script", join(directory, "script.md"),
    "--source", join(root, "application.md"),
    "--out", report,
  ]);
  const verdict = JSON.parse(readFileSync(report, "utf8"));
  const pictures = readdirSync(join(directory, "slides")).filter((file) => file.endsWith(".png"));
  if (verdict.inputs.slides.length !== pictures.length) {
    fail(`${name}: the review names ${verdict.inputs.slides.length} slides for ${pictures.length} pictures`);
  }
  const script = createHash("sha256").update(readFileSync(join(directory, "script.md"))).digest("hex");
  if (verdict.inputs.script.sha256 !== script) fail(`${name}: the review's script digest is not the script's`);
  for (const [dimension, judged] of Object.entries(verdict.evaluation.dimensions)) {
    const evidenced = Array.isArray(judged.evidence) && judged.evidence.some((line) => line.trim());
    if (typeof judged.met !== "boolean" || !evidenced) {
      fail(`${name}: ${dimension} is not judged with evidence`);
    }
  }
  return verdict;
}

const faithful = review("faithful", "1.2");
const misstated = review("misstated", "3.5");
if (misstated.verdict.pass) fail("a talk that misstates its source's cost was passed");
if (misstated.evaluation.dimensions.source_fidelity.met) {
  fail("the misstated cost was judged faithful to the source");
}

rmSync(root, { recursive: true, force: true });
console.log(
  `talk-review: the faithful talk ${faithful.verdict.pass ? "passed" : "was refused"}; the misstated talk was refused for source fidelity`,
);
