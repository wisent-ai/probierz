// Fixtures for the game-asset-creator Probierz app: a synthetic
// on-budget GLB, a synthetic over-budget GLB, and a fake `skarbiec`
// binary (the TUI suite must never touch a real vault).
import { mkdir, writeFile, chmod } from 'node:fs/promises';
import { join } from 'node:path';

export const FIXTURE_DIR = process.env.GAC_FIXTURE_DIR ?? '/tmp/probierz-gac-fixtures';

function makeGlb(json) {
  let jsonStr = JSON.stringify(json);
  while (jsonStr.length % 4 !== 0) jsonStr += ' ';
  const jsonChunk = Buffer.from(jsonStr, 'utf8');
  const header = Buffer.alloc(12);
  header.write('glTF', 0, 'ascii');
  header.writeUInt32LE(2, 4);
  header.writeUInt32LE(12 + 8 + jsonChunk.length, 8);
  const chunkHeader = Buffer.alloc(8);
  chunkHeader.writeUInt32LE(jsonChunk.length, 0);
  chunkHeader.write('JSON', 4, 'ascii');
  return Buffer.concat([header, chunkHeader, jsonChunk]);
}

function modelJson({ triangles, materials = 1 }) {
  return {
    asset: { version: '2.0' },
    accessors: [{ count: triangles * 3 }],
    meshes: [{ primitives: [{ attributes: { POSITION: 0 }, mode: 4 }] }],
    materials: Array.from({ length: materials }, () => ({})),
  };
}

export async function buildFixtures(dir = FIXTURE_DIR) {
  await mkdir(dir, { recursive: true });

  const valid = join(dir, 'valid-6k.glb');
  await writeFile(valid, makeGlb(modelJson({ triangles: 6000 })));

  const overBudget = join(dir, 'over-budget.glb');
  await writeFile(overBudget, makeGlb(modelJson({ triangles: 99999 })));

  const corrupt = join(dir, 'corrupt.glb');
  await writeFile(corrupt, Buffer.from('definitely not a glb file'));

  const fakeSkarbiec = join(dir, 'skarbiec');
  await writeFile(
    fakeSkarbiec,
    `#!/bin/sh
if [ "$1" = "get" ]; then
  case "$2" in
    TEXT2GAME_ACCOUNT) echo '{"fields":{"login_email":"fixture@example.com","login_password":"fixture"}}' ;;
    BRAMA) echo '{"fields":{"agent_auth_secret":"fixture-brama-key"}}' ;;
    *) echo "item not found: $2" >&2; exit 1 ;;
  esac
  exit 0
fi
echo "unknown command: $1" >&2
exit 1
`,
  );
  await chmod(fakeSkarbiec, 0o755);

  const config = join(dir, 'pipeline.config.json');
  await writeFile(
    config,
    JSON.stringify(
      {
        browser: { headless: true },
        credentials: {
          username: 'skarbiec://TEXT2GAME_ACCOUNT/login_email',
          password: 'skarbiec://TEXT2GAME_ACCOUNT/login_password',
        },
        models: {
          brama: {
            url: 'https://model-router.example',
            key: 'skarbiec://BRAMA/agent_auth_secret',
            model: 'any',
          },
        },
        studio: {
          loginUrl: 'https://studio.example/login',
          generateUrl: 'https://studio.example/generate',
          selectors: {
            loginUser: '#u',
            loginPassword: '#p',
            loginSubmit: '#go',
            promptInput: '#prompt',
            generateSubmit: '#gen',
          },
          artifact: { pollExpression: 'null', timeoutMs: 1000, intervalMs: 100 },
        },
        verify: { enabled: true, triTarget: 6000, triTolerancePct: 100 },
      },
      null,
      2,
    ),
  );

  return { dir, valid, overBudget, corrupt, fakeSkarbiec, config };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const fixtures = await buildFixtures();
  console.log(JSON.stringify(fixtures, null, 2));
}
