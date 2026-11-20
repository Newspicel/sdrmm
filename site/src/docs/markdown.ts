import { renderPatchSvg } from "./patch-diagram.ts";

const CALLOUTS: Record<string, { kind: string; title: string }> = {
  NOTE: { kind: "note", title: "Note" },
  TIP: { kind: "tip", title: "Tip" },
  IMPORTANT: { kind: "important", title: "Important" },
  WARNING: { kind: "warning", title: "Warning" },
  CAUTION: { kind: "caution", title: "Caution" },
};

function normalizePosix(path: string): string {
  const parts = path.split("/");
  const stack: string[] = [];
  for (const part of parts) {
    if (part === "" || part === ".") {
      continue;
    }
    if (part === "..") {
      stack.pop();
    } else {
      stack.push(part);
    }
  }
  return stack.join("/");
}

function dirnamePosix(path: string): string {
  const idx = path.lastIndexOf("/");
  return idx === -1 ? "" : path.slice(0, idx);
}

export function rewriteDocLink(href: string, fromFileRelToDocs: string): string {
  if (/^(https?:|mailto:|#|\/)/.test(href)) {
    return href;
  }
  const [pathPart, hashPart] = href.split("#");
  if (!pathPart || !pathPart.endsWith(".md")) {
    return href;
  }
  const fromDir = dirnamePosix(fromFileRelToDocs.replace(/\\/g, "/"));
  const joined = normalizePosix(fromDir ? `${fromDir}/${pathPart}` : pathPart);
  const stripped = joined.replace(/\.md$/, "");
  let target: string;
  if (stripped === "index" || stripped === "") {
    target = "/docs/";
  } else if (stripped.endsWith("/index")) {
    target = `/docs/${stripped.slice(0, -"/index".length)}`;
  } else {
    target = `/docs/${stripped}`;
  }
  return hashPart ? `${target}#${hashPart}` : target;
}

export function preprocessMarkdown(
  markdown: string,
  relToDocs: string,
  readRepoFile?: (relPath: string) => string,
): string {
  const cleaned =
    relToDocs.replace(/\\/g, "/") === "operate/decoders.md"
      ? markdown.replace(/## Catalog\s*\r?\n+<!-- generated[\s\S]*?<!-- end generated -->\s*/m, "")
      : markdown;
  const lines = cleaned.split(/\r?\n/);
  const out: string[] = [];
  let inFence = false;
  let fenceLang = "";
  let fenceLines: string[] = [];

  for (const line of lines) {
    if (!inFence) {
      const open = line.match(/^```([a-zA-Z0-9_-]*)\s*$/);
      if (open) {
        inFence = true;
        fenceLang = open[1] ?? "";
        fenceLines = [];
        continue;
      }
      out.push(
        line.replace(/\]\(([^)\s]+)\)/g, (full, href: string) => {
          const rewritten = rewriteDocLink(href, relToDocs);
          return rewritten === href ? full : `](${rewritten})`;
        }),
      );
      continue;
    }

    if (/^```\s*$/.test(line)) {
      const body = fenceLines.join("\n");
      if (fenceLang === "patch") {
        out.push(renderPatchSvg(body));
      } else {
        const expanded = readRepoFile
          ? body.replace(/\{\{#include\s+([^}]+)\}\}/g, (_, relPath: string) =>
              readRepoFile(relPath.trim()).trimEnd(),
            )
          : body;
        out.push(`\`\`\`${fenceLang}`, expanded, "```");
      }
      inFence = false;
      fenceLang = "";
      fenceLines = [];
      continue;
    }

    fenceLines.push(line);
  }

  if (inFence) {
    out.push(`\`\`\`${fenceLang}`, ...fenceLines);
  }
  return out.join("\n");
}

export function postprocessHtml(html: string): string {
  const withCallouts = html.replace(
    /<blockquote>\s*<p>\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\](?:<br\s*\/?>|\s)*([\s\S]*?)<\/blockquote>/g,
    (full, rawKind: string, rest: string) => {
      const meta = CALLOUTS[rawKind] ?? CALLOUTS.NOTE;
      if (!meta) {
        return full;
      }
      const cleaned = rest.replace(/^\s*<\/p>\s*/, "");
      const body = cleaned === rest ? `<p>${rest}` : cleaned;
      return `<aside class="callout callout-${meta.kind}"><p class="callout-title">${meta.title}</p>${body}</aside>`;
    },
  );
  return withCallouts.replace(
    /<table>[\s\S]*?<\/table>/g,
    (table) => `<div class="table-wrap">${table}</div>`,
  );
}
