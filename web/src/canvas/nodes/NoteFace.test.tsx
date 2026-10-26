import { describe, expect, it } from "vitest";
import type { PatchNode } from "../../lib/types";
import { renderFace } from "../../test/faceHarness";
import { placed } from "../../test/fixtures";
import { NoteFace } from "./NoteFace";

function note(text: string): PatchNode {
  return placed("hint", { kind: "note", data: { text } });
}

function render(text: string): string {
  const node = note(text);
  return renderFace(NoteFace, node, { graph: { nodes: [node], edges: [] }, resolved: null });
}

describe("NoteFace", () => {
  it("asks for text while the note is blank", () => {
    expect(render("  ")).toContain("Double-click to write markdown");
  });

  it("renders the markdown it was given", () => {
    const markup = render("# Pass plan\n\n- **145.800** FM\n- [docs](https://sdrmm.app)");
    expect(markup).toContain("<h1");
    expect(markup).toContain("Pass plan");
    expect(markup).toContain("<strong");
    expect(markup).toContain('href="https://sdrmm.app"');
  });

  it("leaves no link behind for a target that is not a web address", () => {
    const markup = render("[run](javascript:alert)");
    expect(markup).not.toContain("javascript:alert");
    expect(markup).toContain("run");
  });
});
