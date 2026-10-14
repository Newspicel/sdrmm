import { readdir, readFile } from "node:fs/promises";
import { join, relative } from "node:path";

const sourceRoot = new URL("../src/", import.meta.url);

const RULES = [
  {
    pattern: /\btext-\[\d+(?:\.\d+)?(?:px|rem|em)\]/g,
    fix: "use a type token: text-plot, text-2xs, text-xs, text-sm or text-dial-*",
  },
  {
    pattern: /\btext-3xs\b/g,
    fix: "text-3xs merged into text-2xs",
  },
  {
    pattern:
      /\b(?:bg|text|border(?:-[trblxy])?|ring|fill|stroke|outline|from|via|to|divide|decoration|placeholder|caret|shadow)-[a-z][a-z0-9-]*\/(?!(?:5|10|15|25|40|60|85|100)\b)\d+\b/g,
    fix: "color opacity steps: 5, 10, 15, 25, 40, 60 or 85",
  },
  {
    pattern: /(?<![\w/-])opacity-(?!(?:0|45|60|100)\b)\d+\b/g,
    fix: "opacity steps: opacity-0, opacity-45, opacity-60 or opacity-100",
  },
  {
    pattern: /\bfont-(?:thin|extralight|light|bold|extrabold|black)\b/g,
    fix: "font weights: font-normal, font-medium or font-semibold",
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
