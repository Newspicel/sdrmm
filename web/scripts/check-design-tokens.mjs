import { readdir, readFile } from "node:fs/promises";
import { join, relative } from "node:path";

const sourceRoot = new URL("../src/", import.meta.url);

const RULES = [
  {
    pattern: /\btext-\[\d+(?:\.\d+)?(?:px|rem|em)\]/g,
    fix: "use a type token: text-plot, text-3xs, text-2xs, text-xs, text-sm or text-dial-*",
  },
  {
    pattern: /\brounded(?:-[a-z]{1,2})?-\[[^\]]*\]/g,
    fix: "use rounded-control, rounded-inset or rounded-full",
  },
  {
    pattern:
      /(?<=[\s"'`:])rounded(?:-(?:[trblse]|[trbl][lr]|ss|se|es|ee|x|y))?(?:-(?:xs|sm|md|lg|xl|2xl|3xl|4xl))?(?=[\s"'`])(?!\s*[=;),.])/g,
    fix: "use rounded-control, rounded-inset or rounded-full",
  },
  {
    pattern: /\b(?:disabled|data-disabled|data-\[disabled\]|aria-disabled):opacity-(?!45\b)\d+/g,
    fix: "disabled controls fade to opacity-45",
  },
];

const violations = [];

function lineOf(text, index) {
  return text.slice(0, index).split("\n").length;
}

function check(path, text) {
  for (const { pattern, fix } of RULES) {
    for (const match of text.matchAll(pattern)) {
      const where = `${relative(sourceRoot.pathname, path)}:${lineOf(text, match.index)}`;
      violations.push(`${where}: ${match[0]} (${fix})`);
    }
  }
}

async function inspect(directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== "generated") {
        await inspect(path);
      }
    } else if (/\.tsx?$/.test(entry.name) && !entry.name.includes(".test.")) {
      check(path, await readFile(path, "utf8"));
    }
  }
}

await inspect(sourceRoot.pathname);

if (violations.length > 0) {
  console.error("Use the design tokens from index.css:");
  for (const violation of violations) {
    console.error(`  ${violation}`);
  }
  process.exitCode = 1;
}
