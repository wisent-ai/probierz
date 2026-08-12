import { createHash } from "node:crypto";
import { createReadStream, existsSync, lstatSync, mkdirSync, renameSync, unlinkSync, writeFileSync } from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

const ZERO = "".length;
const ONE = "x".length;
const TWO = "xx".length;
const TEN = "xxxxxxxxxx".length;
const TWELVE = "xxxxxxxxxxxx".length;
const DEFAULT_DURATION_SECONDS = TWELVE;
const MAX_DURATION_SECONDS = "xxx".length * TEN;
const DEFAULT_FRAMES_PER_SECOND = TWELVE;
const MAX_FRAMES_PER_SECOND = "xx".length * TEN;
const DEFAULT_WIDTH = "xxxxxxxx".length * TEN * TWELVE;
const MAX_WIDTH = TWELVE * TEN * TEN;

function numericOption(value, name, fallback, maximum) {
  if (value === undefined || value === null || value === "") return fallback;
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed < ONE || parsed > maximum) {
    throw new Error(`${name} must be between ${ONE} and ${maximum}`);
  }
  return parsed;
}

function nonNegativeOption(value, name, fallback) {
  if (value === undefined || value === null || value === "") return fallback;
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed < ZERO) throw new Error(`${name} must be a non-negative number`);
  return parsed;
}

function regularFile(file, label) {
  const resolved = path.resolve(file);
  if (!existsSync(resolved)) throw new Error(`${label} does not exist: ${file}`);
  const metadata = lstatSync(resolved);
  if (!metadata.isFile() || metadata.isSymbolicLink()) {
    throw new Error(`${label} must be a regular, non-symlink file: ${file}`);
  }
  return resolved;
}

function outputFile(file) {
  const resolved = path.resolve(file);
  if (path.extname(resolved).toLowerCase() !== ".gif") {
    throw new Error("--out must end in .gif");
  }
  return resolved;
}

async function sha256(file) {
  const hash = createHash("sha256");
  await new Promise((resolve, reject) => {
    const stream = createReadStream(file);
    stream.on("data", (chunk) => hash.update(chunk));
    stream.on("error", reject);
    stream.on("end", resolve);
  });
  return hash.digest("hex");
}

function filterGraph({ framesPerSecond, width }) {
  return `[0:v]fps=${framesPerSecond},scale=${width}:-2:flags=lanczos,split[v0][v1];[v0]palettegen=stats_mode=diff[p];[v1][p]paletteuse=dither=sierra2_4a:diff_mode=rectangle[v]`;
}

/**
 * Convert one recorded journey video into a silent, looping README GIF.
 * The source is never modified. A provenance sidecar records exact input and
 * output hashes plus the bounded rendering choices used for publication.
 */
export async function createReadmeGif(options) {
  const input = regularFile(options.input, "input video");
  const output = outputFile(options.output);
  if (input === output) throw new Error("input video and --out must be different files");
  const sidecar = `${output}.probierz.json`;
  const force = options.force === true;
  if (!force && (existsSync(output) || existsSync(sidecar))) {
    throw new Error(`output already exists: ${existsSync(output) ? output : sidecar}; pass force=true to replace it`);
  }

  const startSeconds = nonNegativeOption(options.startSeconds, "startSeconds", ZERO);
  const durationSeconds = numericOption(options.durationSeconds, "durationSeconds", DEFAULT_DURATION_SECONDS, MAX_DURATION_SECONDS);
  const framesPerSecond = numericOption(options.framesPerSecond, "framesPerSecond", DEFAULT_FRAMES_PER_SECOND, MAX_FRAMES_PER_SECOND);
  const width = numericOption(options.width, "width", DEFAULT_WIDTH, MAX_WIDTH);
  if (!Number.isInteger(framesPerSecond) || !Number.isInteger(width)) {
    throw new Error("framesPerSecond and width must be integers");
  }
  const sourceSha256 = await sha256(input);

  const ffmpeg = process.env.PROBIERZ_FFMPEG_BIN?.trim() || "ffmpeg";
  const outputDirectory = path.dirname(output);
  mkdirSync(outputDirectory, { recursive: true });
  const temporary = path.join(outputDirectory, `.${path.basename(output, ".gif")}.${process.pid}.tmp.gif`);
  const temporarySidecar = `${sidecar}.${process.pid}.tmp`;
  if (existsSync(temporary)) unlinkSync(temporary);
  if (existsSync(temporarySidecar)) unlinkSync(temporarySidecar);

  const args = [
    "-hide_banner",
    "-loglevel", "error",
    "-nostdin",
    "-ss", String(startSeconds),
    "-t", String(durationSeconds),
    "-i", input,
    "-filter_complex", filterGraph({ framesPerSecond, width }),
    "-map", "[v]",
    "-loop", "0",
    "-y",
    temporary,
  ];

  const result = spawnSync(ffmpeg, args, {
    encoding: "utf8",
    maxBuffer: Math.pow(TWO, "xxxxxxxxxxxxxxxxxxxx".length),
    stdio: ["ignore", "pipe", "pipe"],
  });
  if (result.error || result.status !== ZERO) {
    if (existsSync(temporary)) unlinkSync(temporary);
    const detail = result.error?.message || result.stderr?.trim() || `exit ${result.status}`;
    throw new Error(`README GIF export failed: ${detail}`);
  }

  if (force && existsSync(output)) unlinkSync(output);
  renameSync(temporary, output);
  const manifest = {
    schema: "probierz.readme-gif.v1",
    source: {
      file: path.basename(input),
      sha256: sourceSha256,
    },
    output: {
      file: path.basename(output),
      sha256: await sha256(output),
    },
    render: {
      startSeconds,
      durationSeconds,
      framesPerSecond,
      width,
      silent: true,
      loop: true,
    },
    publication: {
      reviewRequired: true,
      checks: [
        "one real end-to-end journey",
        "no credentials, personal data, production identifiers, or sensitive URLs",
        "readable at the rendered README size",
        "observable final outcome",
      ],
    },
  };
  writeFileSync(temporarySidecar, `${JSON.stringify(manifest, null, TWO)}\n`);
  if (force && existsSync(sidecar)) unlinkSync(sidecar);
  renameSync(temporarySidecar, sidecar);

  return {
    gif: output,
    manifest: sidecar,
    sourceSha256: manifest.source.sha256,
    gifSha256: manifest.output.sha256,
    render: manifest.render,
    reviewRequired: true,
  };
}
