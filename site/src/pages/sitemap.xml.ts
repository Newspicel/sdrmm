import type { APIRoute } from "astro";
import { FLAT_DOCS } from "../docs/nav";
import { docPages, sitemap, sitePages } from "../seo";

const pages = Object.keys(import.meta.glob("./*.astro"));

export const GET: APIRoute = () =>
  new Response(sitemap([...sitePages(pages), ...docPages(FLAT_DOCS.map((doc) => doc.slug))]), {
    headers: { "content-type": "application/xml" },
  });
