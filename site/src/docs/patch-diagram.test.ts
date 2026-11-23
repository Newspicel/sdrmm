import { describe, expect, it } from "vitest";
import { layoutPatch, parsePatch, type Rect } from "./patch-diagram";

const RAW_DOCS = import.meta.glob<string>(
  ["../../../docs/**/*.md", "!../../../docs/superpowers/**"],
  {
    query: "?raw",
    import: "default",
    eager: true,
  },
);

function overlaps(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;
}

describe("patch diagram layout", () => {
  it("routes every patch diagram in docs without tag-on-node collisions", () => {
    let checked = 0;
    for (const [file, content] of Object.entries(RAW_DOCS)) {
      const blocks = content.matchAll(/```patch\r?\n([\s\S]*?)```/g);
      for (const match of blocks) {
        checked += 1;
        const spec = parsePatch(match[1] ?? "");
        const layout = layoutPatch(spec);
        const nodeRects: Rect[] = layout.nodes.map((n) => ({ x: n.x, y: n.y, w: n.w, h: n.h }));
        for (const tag of layout.tags) {
          for (const nodeRect of nodeRects) {
            expect(
              overlaps(tag.rect, nodeRect),
              `${file}: tag "${tag.text}" overlapped a node box`,
            ).toBe(false);
          }
        }
        for (let i = 0; i < layout.tags.length; i += 1) {
          for (let j = i + 1; j < layout.tags.length; j += 1) {
            const a = layout.tags[i];
            const b = layout.tags[j];
            if (!a || !b) continue;
            expect(
              overlaps(a.rect, b.rect),
              `${file}: tags "${a.text}" and "${b.text}" overlapped`,
            ).toBe(false);
          }
        }
      }
    }
    expect(checked).toBeGreaterThanOrEqual(10);
  });
});
