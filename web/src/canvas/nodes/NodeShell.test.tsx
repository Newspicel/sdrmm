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

function render(content: string): string {
  return renderToStaticMarkup(
    <QueryClientProvider client={new QueryClient()}>
      <ReactFlowProvider>
        <WorkspaceProvider
          value={stubWorkspace({ graph: { nodes: [NODE], edges: [] }, resolved: null })}
        >
          <CanvasSurface>
            <NodeShell node={NODE} title="Scanner" category="tool">
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
});
