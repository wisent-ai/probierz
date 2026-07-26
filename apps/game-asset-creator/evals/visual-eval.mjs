// visual-eval.mjs — visual evaluation ("ocena") of game assets, Probierz-side.
//
// For every GLB in the input set: render fixed-angle frames through the
// Blender MCP session, then score the montage with a VISION MODEL THROUGH
// BRAMA (the only model path — same rule as the pipeline itself) using a
// rubric. Writes eval-report.json (+ renders) into the artifacts dir.
//
// Usage:
//   node apps/game-asset-creator/evals/visual-eval.mjs \
//     --models <dir-with-glbs> [--out <artifacts-dir>] \
//     [--config <pipeline.config.json>] [--rubric rts-character] \
//     [--threshold 0.7]
//
// Credentials: the Brama key resolves from Skarbiec through the gac
// pipeline config loader — never from env.

import { mkdir, readdir, readFile, writeFile } from 'node:fs/promises';
import { join, basename } from 'node:path';

const GAC_ROOT = process.env.GAC_ROOT ?? '/Users/lukaszbartoszcze/work/game_asset_creator';
const { BlenderSession } = await import(`${GAC_ROOT}/pipeline/blender.js`);
const { loadPipelineConfig } = await import(`${GAC_ROOT}/pipeline/config.js`);

const RUBRICS = {
  'rts-character': `You are an art director reviewing ONE low-poly RTS character render set.
Score each dimension 0..1 and give an overall score 0..1:
- proportions: chunky heroic low-poly (Thronefall style), not noodle-limbed
- silhouette: readable at RTS camera distance, clear head/torso/weapon shapes
- palette: flat-shaded colors consistent with a fantasy race (no texture noise)
- artifacts: no z-fighting, no missing limbs, no collapsed geometry
Reply with a single JSON object: {"proportions": x, "silhouette": x, "palette": x, "artifacts": x, "overall": x, "issues": ["..."]}`,
};

const ANGLES = [
  { name: 'front', rot: '(0, 0, 0)' },
  { name: 'side', rot: '(0, 0, 1.5708)' },
  { name: 'back34', rot: '(0, 0, 3.927)' },
];

function parseArgs(argv) {
  const options = {};
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i].startsWith('--')) options[argv[i].slice(2)] = argv[i + 1] ?? true;
    if (argv[i].startsWith('--')) i += 1;
  }
  return options;
}

async function renderAngles(session, glbPath, outDir) {
  const paths = [];
  for (const angle of ANGLES) {
    const out = join(outDir, `${basename(glbPath, '.glb')}-${angle.name}.png`);
    const code = [
      'import bpy',
      'bpy.ops.wm.read_factory_settings(use_empty=True)',
      `bpy.ops.import_scene.gltf(filepath=${JSON.stringify(glbPath)})`,
      'scene = bpy.context.scene',
      'for obj in scene.objects: obj.rotation_euler = ' + angle.rot,
      'scene.render.engine = "BLENDER_EEVEE_NEXT" if hasattr(bpy.types, "BLENDER_EEVEE_NEXT") else "BLENDER_EEVEE"',
      'scene.render.resolution_x = 512',
      'scene.render.resolution_y = 512',
      `scene.render.filepath = ${JSON.stringify(out)}`,
      'bpy.ops.render.render(write_still=True)',
      'import os',
      `print("rendered", os.path.getsize(${JSON.stringify(out)}))`,
    ].join('\n');
    await session.execute(code);
    paths.push(out);
  }
  return paths;
}

async function scoreWithBrama({ url, key, model, rubric, imagePaths }) {
  const content = [{ type: 'text', text: rubric }];
  for (const path of imagePaths) {
    const png = await readFile(path);
    content.push({
      type: 'image_url',
      image_url: { url: `data:image/png;base64,${png.toString('base64')}` },
    });
  }
  const response = await fetch(`${url.replace(/\/+$/, '')}/v1/chat/completions`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', authorization: `Bearer ${key}` },
    body: JSON.stringify({
      model: model ?? 'any',
      max_tokens: 1024,
      messages: [{ role: 'user', content }],
    }),
  });
  const body = await response.json().catch(() => ({}));
  if (!response.ok) {
    throw new Error(`brama HTTP ${response.status}: ${body?.error?.message ?? 'unknown'}`);
  }
  const text = body.choices?.[0]?.message?.content ?? '';
  const start = text.indexOf('{');
  const end = text.lastIndexOf('}');
  if (start === -1 || end === -1) throw new Error(`brama reply had no JSON: ${text.slice(0, 200)}`);
  return JSON.parse(text.slice(start, end + 1));
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const modelsDir = args.models ?? `${GAC_ROOT}/assets/models`;
  const outDir = args.out ?? join(process.cwd(), 'test-results', 'visual-eval');
  const configPath = args.config ?? `${GAC_ROOT}/pipeline.config.json`;
  const rubricName = args.rubric ?? process.env.PROBIERZ_EVAL_RUBRIC ?? 'rts-character';
  const threshold = Number(args.threshold ?? 0.7);

  const config = await loadPipelineConfig(configPath);
  const brama = config.models?.brama;
  if (!brama?.url || !brama?.key) {
    throw new Error('models.brama.{url,key} missing from pipeline config (skarbiec:// refs)');
  }

  const glbs = (await readdir(modelsDir)).filter((f) => f.endsWith('.glb')).sort();
  if (glbs.length === 0) throw new Error(`no .glb files in ${modelsDir}`);

  await mkdir(outDir, { recursive: true });
  const session = await BlenderSession.start(config.blender?.mcp ?? {});

  const results = [];
  try {
    for (const glb of glbs) {
      const glbPath = join(modelsDir, glb);
      const renders = await renderAngles(session, glbPath, outDir);
      const scores = await scoreWithBrama({
        url: brama.url,
        key: brama.key,
        model: brama.model,
        rubric: RUBRICS[rubricName] ?? rubricName,
        imagePaths: renders,
      });
      results.push({
        asset: glb,
        renders,
        scores,
        pass: typeof scores.overall === 'number' && scores.overall >= threshold,
      });
    }
  } finally {
    await session.close().catch(() => {});
  }

  const passed = results.filter((r) => r.pass).length;
  const report = {
    rubric: rubricName,
    threshold,
    total: results.length,
    passed,
    failed: results.length - passed,
    results,
  };
  const reportPath = join(outDir, 'eval-report.json');
  await writeFile(reportPath, JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ reportPath, total: report.total, passed, failed: report.failed }, null, 2));
  if (report.failed > 0) process.exitCode = 1;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((error) => {
    console.error(`visual-eval failed: ${error.message}`);
    process.exitCode = 1;
  });
}
