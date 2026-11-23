import { describe, expect, it } from "vitest";
import { CATALOG } from "../modes";
import { postprocessHtml, preprocessMarkdown, rewriteDocLink } from "./markdown";
import { FLAT_DOCS, normalizeDocId } from "./nav";
import { parsePatch, renderPatchSvg } from "./patch-diagram";

const rawDocs = import.meta.glob<string>(
  ["../../../docs/**/*.md", "!../../../docs/superpowers/**"],
  {
    query: "?raw",
    import: "default",
    eager: true,
  },
);

const docsByRel = new Map<string, string>(
  Object.entries(rawDocs).map(([key, content]) => [key.replace(/^.*?\/docs\//, ""), content]),
);

const fileBySlug = new Map<string, string>(
  [...docsByRel.keys()].map((rel) => [normalizeDocId(rel), rel]),
);

function normalizeRel(fromRel: string, target: string): string {
  const dir = fromRel.includes("/") ? fromRel.slice(0, fromRel.lastIndexOf("/")) : "";
  const combined = dir ? `${dir}/${target}` : target;
  const stack: string[] = [];
  for (const part of combined.split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") stack.pop();
    else stack.push(part);
  }
  return stack.join("/");
}

function slugifyHeading(text: string): string {
  return text
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_\s-]/g, "")
    .replace(/ /g, "-");
}

function headingsIn(markdown: string): Set<string> {
  const withoutCode = markdown.replace(/```[\s\S]*?```/g, "");
  const counts = new Map<string, number>();
  const found = new Set<string>();
  for (const match of withoutCode.matchAll(/^#{2,4}\s+(.+)$/gm)) {
    const base = slugifyHeading(match[1] ?? "");
    const seen = counts.get(base) ?? 0;
    counts.set(base, seen + 1);
    found.add(seen === 0 ? base : `${base}-${seen}`);
  }
  return found;
}

describe("docs navigation and files", () => {
  it("matches every markdown file with the sidebar navigation", () => {
    const navSlugs = new Set(FLAT_DOCS.map((item) => item.slug));
    const fileSlugs = new Set(fileBySlug.keys());

    expect(fileSlugs.size).toBeGreaterThan(20);
    for (const slug of navSlugs) {
      expect(fileSlugs, `nav entry '${slug}' has no markdown file`).toContain(slug);
    }
    for (const slug of fileSlugs) {
      expect(navSlugs, `docs file for '${slug}' is missing from DOCS_NAV`).toContain(slug);
    }
  });

  it("gives every page YAML frontmatter with a title and no em dashes", () => {
    for (const [rel, text] of docsByRel) {
      expect(text, `${rel} must start with frontmatter`).toMatch(
        /^---\n(?:[\s\S]*?\n)?title:\s*.+\n[\s\S]*?\n---\n/,
      );
      expect(text, `${rel} must not contain an em dash`).not.toContain("\u2014");
    }
  });

  it("resolves every relative markdown link and heading anchor", () => {
    const headingIndex = new Map<string, Set<string>>();
    for (const [slug, rel] of fileBySlug) {
      headingIndex.set(slug, headingsIn(docsByRel.get(rel) ?? ""));
    }

    for (const [rel, raw] of docsByRel) {
      const slug = normalizeDocId(rel);
      const text = raw.replace(/```[\s\S]*?```/g, "");
      for (const match of text.matchAll(/\]\(([^)\s]+)\)/g)) {
        const href = match[1] ?? "";
        if (/^(https?:|mailto:|\/)/.test(href)) {
          continue;
        }
        const [rawPath, hash] = href.split("#");
        let targetSlug = slug;
        if (rawPath && rawPath !== "") {
          expect(rawPath, `${rel} links to non-markdown relative path ${href}`).toMatch(/\.md$/);
          const joined = normalizeRel(rel, rawPath);
          targetSlug = normalizeDocId(joined);
          expect(fileBySlug.has(targetSlug), `${rel} -> ${href} does not exist`).toBe(true);
        }
        if (hash) {
          const anchors = headingIndex.get(targetSlug);
          expect(
            anchors?.has(hash),
            `${rel} -> ${href} targets missing anchor #${hash} on '${targetSlug}'`,
          ).toBe(true);
        }
      }
    }
  });

  it("resolves every mode doc link in modes.json", () => {
    const headingIndex = new Map<string, Set<string>>();
    for (const [slug, rel] of fileBySlug) {
      headingIndex.set(slug, headingsIn(docsByRel.get(rel) ?? ""));
    }
    const linked = CATALOG.modes.filter((mode) => mode.doc !== undefined);
    expect(linked.length).toBeGreaterThan(20);
    for (const mode of linked) {
      const doc = mode.doc ?? "";
      const [pathPart, hash] = doc.split("#");
      const slug = (pathPart ?? "").replace(/^\/docs\/?/, "");
      expect(fileBySlug.has(slug), `${mode.name} -> ${doc}`).toBe(true);
      if (hash) {
        expect(headingIndex.get(slug)?.has(hash), `${mode.name} -> ${doc}`).toBe(true);
      }
    }
  });
});

describe("patch diagram renderer", () => {
  it("parses nodes and typed wires and renders accessible SVG", () => {
    const spec = parsePatch(`
      [source] Device : RTL-SDR
      [channel] WFM : 100 MHz
      [output] Speaker
      Device --iq--> WFM
      WFM --audio--> Speaker
    `);
    expect(spec.nodes).toHaveLength(3);
    expect(spec.edges).toHaveLength(2);
    const svg = renderPatchSvg(`
      [source] Device : RTL-SDR
      [channel] WFM : 100 MHz
      Device --iq--> WFM
    `);
    expect(svg).toContain('class="patch-diagram"');
    expect(svg).toContain("pd-port-iq");
    expect(svg).toContain("RTL-SDR");
  });
});

describe("markdown preprocessing and postprocessing", () => {
  it("rewrites relative markdown links into clean /docs/ URLs", () => {
    expect(rewriteDocLink("../operate/channels.md#tuning", "start/first-receiver.md")).toBe(
      "/docs/operate/channels#tuning",
    );
    expect(rewriteDocLink("index.md", "radios/usb.md")).toBe("/docs/radios");
    expect(rewriteDocLink("../index.md", "start/install.md")).toBe("/docs/");
    expect(rewriteDocLink("https://sdrmm.com/modes", "index.md")).toBe("https://sdrmm.com/modes");
  });

  it("renders patch fences and expands code includes", () => {
    const out = preprocessMarkdown(
      "See [Channels](../operate/channels.md).\n\n```patch\n[source] Device\n[output] Scope\nDevice --iq--> Scope\n```\n\n```c\n{{#include demo.c}}\n```\n",
      "start/first-receiver.md",
      () => "int main(void) { return 0; }\n",
    );
    expect(out).toContain("[Channels](/docs/operate/channels)");
    expect(out).toContain('class="patch-diagram"');
    expect(out).toContain("int main(void)");
  });

  it("transforms callouts and wraps tables in HTML", () => {
    const html = postprocessHtml(
      "<blockquote><p>[!WARNING]\nWatch out.</p></blockquote><table><tr><td>A</td></tr></table>",
    );
    expect(html).toContain('class="callout callout-warning"');
    expect(html).toContain('<div class="table-wrap"><table>');
  });
});
