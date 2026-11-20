import { readFileSync } from "node:fs";
import { basename, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import react from "@astrojs/react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "astro/config";
import { postprocessHtml, preprocessMarkdown } from "./src/docs/markdown.ts";
import { SITE } from "./src/seo.ts";

const siteDir = fileURLToPath(new URL(".", import.meta.url));
const repoRoot = resolve(siteDir, "..");
const docsRoot = resolve(repoRoot, "docs").replace(/\\/g, "/");

function relativeToDocs(fileURL) {
  if (!fileURL) {
    return "";
  }
  const full = (fileURL.protocol === "file:" ? fileURLToPath(fileURL) : fileURL.pathname).replace(
    /\\/g,
    "/",
  );
  if (full.startsWith(`${docsRoot}/`)) {
    return full.slice(docsRoot.length + 1);
  }
  const marker = "/docs/";
  const idx = full.lastIndexOf(marker);
  return idx >= 0 ? full.slice(idx + marker.length) : basename(full);
}

function docsMarkdown() {
  return {
    name: "sdrmm-docs-markdown",
    hooks: {
      "astro:config:setup"({ config, updateConfig }) {
        const base = config.markdown.processor;
        updateConfig({
          markdown: {
            processor: {
              ...base,
              name: "sdrmm-docs-satteri",
              async createRenderer(shared) {
                const renderer = await base.createRenderer(shared);
                return {
                  async render(content, renderOpts) {
                    const rel = relativeToDocs(renderOpts?.fileURL);
                    const prepared = preprocessMarkdown(content, rel, (relPath) =>
                      readFileSync(resolve(repoRoot, relPath), "utf8"),
                    );
                    const result = await renderer.render(prepared, renderOpts);
                    return {
                      ...result,
                      code: postprocessHtml(result.code),
                    };
                  },
                };
              },
            },
          },
        });
      },
    },
  };
}

export default defineConfig({
  site: SITE,
  compressHTML: true,
  devToolbar: { enabled: false },
  build: { format: "file" },
  markdown: {
    shikiConfig: {
      theme: "github-dark-default",
    },
  },
  integrations: [docsMarkdown(), react({ exclude: [/node_modules/], compiler: true })],
  vite: {
    plugins: [tailwindcss()],
    resolve: { dedupe: ["react", "react-dom"] },
    build: { chunkSizeWarningLimit: 1000 },
    server: { fs: { allow: [".."] } },
  },
});
