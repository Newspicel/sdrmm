import { getCollection } from "astro:content";
import type { APIRoute } from "astro";
import { docHref, findDoc } from "../../docs/nav";
import { CATALOG, familyTitle } from "../../modes";

function stripMarkdown(md: string): string {
  return md
    .replace(/```patch[\s\S]*?```/g, " ")
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/<!--[\s\S]*?-->/g, " ")
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/[>*_|]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function extractHeadings(md: string): string[] {
  return [...md.matchAll(/^#{2,3}\s+(.+)$/gm)].map((match) => (match[1] ?? "").trim());
}

export const GET: APIRoute = async () => {
  const entries = await getCollection("docs");
  const docsItems = entries.map((entry) => {
    const slug = entry.id === "index" ? "" : entry.id;
    const nav = findDoc(slug);
    const body = stripMarkdown(entry.body ?? "");
    return {
      title: entry.data.title,
      section: nav?.section ?? "Docs",
      href: docHref(slug),
      headings: extractHeadings(entry.body ?? ""),
      body: entry.data.description ? `${entry.data.description} ${body}` : body,
    };
  });

  const modeItems = CATALOG.modes.map((mode) => ({
    title: mode.name,
    section: `Mode · ${familyTitle(CATALOG, mode.family)}`,
    href: `/modes#mode-${mode.type_id}`,
    headings: [mode.type_id],
    body: mode.summary,
  }));

  return new Response(JSON.stringify([...docsItems, ...modeItems]), {
    headers: { "content-type": "application/json; charset=utf-8" },
  });
};
