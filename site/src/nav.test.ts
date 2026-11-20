import { describe, expect, it } from "vitest";
import { FLAT_DOCS } from "./docs/nav";
import { COMMUNITY, FOOTER, isCurrent, LEGAL, PRIMARY } from "./nav";
import { canonical, docPages, sitePages } from "./seo";

const pages = Object.keys(import.meta.glob("./pages/*.astro"));
const known = new Set([...sitePages(pages), ...docPages(FLAT_DOCS.map((doc) => doc.slug))]);
const sources = import.meta.glob<string>(
  ["./pages/*.astro", "./pages/docs/*.astro", "./components/*.astro"],
  {
    query: "?raw",
    import: "default",
    eager: true,
  },
);

describe("page sources", () => {
  it("link to a page or a docs chapter", () => {
    const hrefs = Object.values(sources).flatMap((source) =>
      [...source.matchAll(/href="(\/[^"#?]*)/g)].flatMap((match) => match[1] ?? []),
    );
    const internal = hrefs.filter((href) => !/\.[^/]*$/.test(href));
    expect(internal.length).toBeGreaterThan(0);
    for (const href of internal) {
      expect(known, href).toContain(canonical(href).href);
    }
  });
});

describe("links", () => {
  const links = [PRIMARY, COMMUNITY, LEGAL, ...FOOTER.map((group) => group.links)].flat();

  it("point at a page or a docs chapter", () => {
    const internal = links.filter((link) => link.href.startsWith("/"));
    expect(internal.length).toBeGreaterThan(0);
    for (const link of internal) {
      expect(known, link.href).toContain(canonical(link.href.replace(/#.*$/, "")).href);
    }
  });

  it("leave the site over HTTPS only", () => {
    for (const link of links.filter((candidate) => !candidate.href.startsWith("/"))) {
      expect(link.href).toMatch(/^https:\/\//);
    }
  });
});

describe("isCurrent", () => {
  it("matches with or without the extension", () => {
    expect(isCurrent("/download", "/download")).toBe(true);
    expect(isCurrent("/download", "/download.html")).toBe(true);
    expect(isCurrent("/download", "/download/")).toBe(true);
  });

  it("treats the index as the root", () => {
    expect(isCurrent("/", "/index.html")).toBe(true);
  });

  it("highlights Docs and Hardware for nested docs pages", () => {
    expect(isCurrent("/docs/", "/docs/start/install")).toBe(true);
    expect(isCurrent("/docs/", "/docs/radios")).toBe(false);
    expect(isCurrent("/docs/radios", "/docs/radios/usb")).toBe(true);
  });

  it("never marks other pages or external links", () => {
    expect(isCurrent("/business", "/download")).toBe(false);
    expect(isCurrent("https://discord.gg/dYaRyGwBNw", "/")).toBe(false);
  });
});
