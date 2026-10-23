// Journey talk-render: a speaker renders a timed talk with backup slides
// through the installed lem-cli and gets one PDF page per slide, a PNG per
// slide and the speaker script; a talk over its time limit is refused with every
// slide's minutes and leaves no PDF.
//
// Declarations the journey reads, each refused by name when missing:
// TUI_CMD, the lem-cli executable under test; LEM_WISENT_TOKENS, the
// wisent-components tokens.json the talk is set in; LEM_TALK_SLIDE_WIDTH, the
// width in pixels the slide PNGs are asked at.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

function declared(name) {
  const value = process.env[name];
  if (!value) {
    throw new Error(`${name} is not set; the talk-render journey needs it`);
  }
  return value;
}

function count(text, pattern, what) {
  const found = text.match(pattern);
  if (!found) {
    throw new Error(`talk-render: ${what} holds no ${pattern}`);
  }
  return found.length;
}

const lem = declared("TUI_CMD");
const tokens = declared("LEM_WISENT_TOKENS");
const width = declared("LEM_TALK_SLIDE_WIDTH");

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "test-results", `lem-talk-${process.pid}`);
rmSync(root, { recursive: true, force: true });
mkdirSync(root, { recursive: true });

const talk = [
  "---",
  "title: Journey talk",
  "footer: Example Org",
  "duration: 2",
  "---",
  "",
  "<!-- layout: cover -->",
  "<!-- minutes: 1 -->",
  "",
  "# Journey talk",
  "",
  "## A subtitle",
  "",
  "---",
  "",
  "<!-- layout: bullets -->",
  "<!-- minutes: 1 -->",
  "",
  "# Points",
  "",
  "- **Lead** a point",
  "",
  "<!--",
  "The speech.",
  "-->",
].join("\n");
const questions = [
  "<!-- layout: statement -->",
  "<!-- backup -->",
  "",
  "# A question?",
  "",
  "<!--",
  "The prepared answer.",
  "-->",
].join("\n");
const slides = count(talk + questions, /<!-- layout:/g, "the talk");
const backups = count(questions, /<!-- backup -->/g, "the questions");

writeFileSync(join(root, "talk.md"), talk);
writeFileSync(join(root, "questions.md"), questions);

function lemCli(args) {
  return spawnSync(lem, ["talk", ...args], { cwd: root, encoding: "utf8" });
}

function fail(message) {
  throw new Error(`talk-render: ${message}`);
}

const rendered = lemCli([
  "render",
  "--talk", "talk.md",
  "--talk", "questions.md",
  "--output", "talk.pdf",
  "--tokens", tokens,
  "--slides", "slides",
  "--slide-width", width,
  "--notes", "script.md",
]);
if (rendered.error) fail(`lem-cli could not run: ${rendered.error.message}`);
if (rendered.status) fail(`lem-cli talk render exited ${rendered.status}: ${rendered.stderr}`);
const report = JSON.parse(rendered.stdout);
if (report.slides !== slides) fail(`the report counts ${report.slides} slides for ${slides}`);
if (report.backup_slides !== backups) fail(`the report counts ${report.backup_slides} backup slides for ${backups}`);
const pages = count(readFileSync(join(root, "talk.pdf"), "latin1"), /\/Type\s*\/Page\b/g, "talk.pdf");
if (pages !== slides) fail(`talk.pdf holds ${pages} pages for ${slides} slides`);
const pictures = readdirSync(join(root, "slides")).filter((name) => name.endsWith(".png"));
if (pictures.length !== slides) fail(`slides/ holds ${pictures.length} PNGs for ${slides} slides`);
const script = readFileSync(join(root, "script.md"), "utf8");
if (!script.includes("The prepared answer.") || !script.includes("A question? (backup)")) {
  fail(`the speaker script lacks the backup slide's answer:\n${script}`);
}

rmSync(join(root, "talk.pdf"));
writeFileSync(join(root, "long.md"), talk.replace("duration: 2", "duration: 1"));
const refused = lemCli(["render", "--talk", "long.md", "--output", "talk.pdf", "--tokens", tokens]);
const expected =
  "lem-cli: the talk slides take 2 minutes but the talk's duration is 1; shorten slides (minutes by slide: 1: 1, 2: 1)";
if (!refused.status) fail("a talk over its duration was rendered");
if (refused.stderr.trim() !== expected) fail(`the over-duration refusal reads:\n${refused.stderr}`);
if (existsSync(join(root, "talk.pdf"))) fail("a refused talk still wrote its PDF");

rmSync(root, { recursive: true, force: true });
console.log(`talk-render: ${slides} slides rendered; the over-duration talk was refused`);
