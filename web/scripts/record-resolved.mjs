import { execFileSync } from "node:child_process";
import { readdirSync, rmSync } from "node:fs";

const inputs = new URL("../src/test/resolved-inputs/", import.meta.url);
const root = new URL("../../", import.meta.url);
const ROUNDS = 6;

rmSync(inputs, { recursive: true, force: true });
let recorded = -1;
for (let round = 0; round < ROUNDS; round += 1) {
  try {
    execFileSync("pnpm", ["exec", "vitest", "run", "--reporter=dot"], {
      env: { ...process.env, SDRMM_RECORD_RESOLVED: "1" },
      stdio: "ignore",
    });
  } catch {}
  execFileSync("cargo", ["xtask", "codegen"], { cwd: root, stdio: "inherit" });
  const count = readdirSync(inputs).length;
  if (count === recorded) {
    break;
  }
  recorded = count;
}
