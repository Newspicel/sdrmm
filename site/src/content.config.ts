import { defineCollection } from "astro:content";
import { glob } from "astro/loaders";
import { z } from "astro/zod";
import { normalizeDocId } from "./docs/nav";

const docs = defineCollection({
  loader: glob({
    pattern: ["**/*.md", "!superpowers/**"],
    base: "../docs",
    generateId: ({ entry }) => normalizeDocId(entry) || "index",
  }),
  schema: z.object({
    title: z.string(),
    description: z.string().optional(),
  }),
});

export const collections = { docs };
