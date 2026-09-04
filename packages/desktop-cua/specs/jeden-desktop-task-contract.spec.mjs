import { resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const repository = process.env.PROBIERZ_APP_REPO
  || fileURLToPath(new URL("../../../../jeden-desktop/", import.meta.url));
await import(pathToFileURL(resolve(repository, "tests/contracts/task-contract.probierz.spec.mjs")).href);
