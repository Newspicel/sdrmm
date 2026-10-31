import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ReactFlowProvider } from "@xyflow/react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { stubWorkspace } from "../test/faceHarness";
import { placed } from "../test/fixtures";
import { WorkspaceProvider } from "./context";
import { Rack } from "./Rack";

const NOTE = placed("note1", { kind: "note", data: { text: "pinned" } });

function render(): string {
  const workspace = {
    ...stubWorkspace({ graph: { nodes: [NOTE], edges: [] }, resolved: null }),
    rack: { slots: [{ node: NOTE.id, x: 0, y: 0, w: 6, h: 4 }] },
  };
  return renderToStaticMarkup(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <ReactFlowProvider>
        <WorkspaceProvider value={workspace}>
          <Rack />
        </WorkspaceProvider>
      </ReactFlowProvider>
    </QueryClientProvider>,
  );
}

describe("Rack grips", () => {
  it("names every grip so a keyboard can reach it", () => {
    const html = render();
    for (const label of [
      "Move note1",
      "Resize note1",
      "Boundary above note1",
      "Boundary below note1",
      "Boundary left of note1",
      "Boundary right of note1",
    ]) {
      expect(html).toContain(`aria-label="${label}"`);
    }
  });

  it("hides no grip from assistive tech and offers the arrow keys", () => {
    const html = render();
    expect(html).not.toContain('aria-hidden="true" title="Resize"');
    expect(html).toContain('aria-keyshortcuts="ArrowUp ArrowDown ArrowLeft ArrowRight"');
  });

  it("stretches the face over its slot and lets a narrow slot scroll", () => {
    const html = render();
    expect(html).not.toMatch(/style="width:/);
    expect(html).toMatch(
      /class="size-full overflow-auto"><div class="h-full" style="min-width:\d+px"/,
    );
  });
});
