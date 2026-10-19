import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ReactFlowProvider } from "@xyflow/react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { PatchNode } from "../../lib/types";
import { stubWorkspace } from "../../test/faceHarness";
import { placed } from "../../test/fixtures";
import { WorkspaceProvider } from "../context";
import { CanvasSurface, NodeShell } from "./NodeShell";

const NODE: PatchNode = placed("scan1", { kind: "scanner", data: {} });

function render(content: string, over: { selected?: string; live?: boolean } = {}): string {
  return renderToStaticMarkup(
    <QueryClientProvider client={new QueryClient()}>
      <ReactFlowProvider>
        <WorkspaceProvider
          value={stubWorkspace({
            graph: { nodes: [NODE], edges: [] },
            resolved: null,
            selected: over.selected ?? null,
          })}
        >
          <CanvasSurface>
            <NodeShell node={NODE} title="Scanner" category="tool" live={over.live}>
              {content}
            </NodeShell>
          </CanvasSurface>
        </WorkspaceProvider>
      </ReactFlowProvider>
    </QueryClientProvider>,
  );
}

describe("NodeShell on the canvas", () => {
  it("keeps a content-sized face at one width whatever it shows", () => {
    for (const markup of [render("idle"), render("scanning ".repeat(40))]) {
      expect(markup).toContain("width:400px");
      expect(markup).not.toContain("w-max");
    }
  });

  it("carries the accent along the top bar of the selected face", () => {
    expect(render("idle", { selected: NODE.id })).toContain(
      "shadow-[inset_0_2px_0_var(--color-accent)]",
    );
    expect(render("idle")).toContain("shadow-[inset_0_2px_0_var(--color-cat-tool)]");
  });

  it("tints the top bar of a face that is passing audio", () => {
    expect(render("idle", { live: true })).toContain("bg-ok/25");
    expect(render("idle")).toContain("bg-cat-tool/15");
  });
});
