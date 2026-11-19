import type { ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { stubWorkspace } from "../test/faceHarness";
import { WorkspaceProvider } from "./context";
import { InlineInspector, Inspect, NoInspect } from "./inspector";

function render(inspected: string | null, children: ReactNode): string {
  return renderToStaticMarkup(
    <WorkspaceProvider
      value={{ ...stubWorkspace({ graph: { nodes: [], edges: [] }, resolved: null }), inspected }}
    >
      <InlineInspector>{children}</InlineInspector>
    </WorkspaceProvider>,
  );
}

describe("Inspect", () => {
  it("draws only for the inspected node", () => {
    const content = <Inspect node="a">Gain</Inspect>;
    expect(render("a", content)).toBe("Gain");
    expect(render("b", content)).toBe("");
    expect(render(null, content)).toBe("");
  });

  it("leaves the panel to the first face of a node", () => {
    expect(
      render(
        "a",
        <NoInspect>
          <Inspect node="a">Gain</Inspect>
        </NoInspect>,
      ),
    ).toBe("");
  });

  it("keeps the head slot out of an inline face", () => {
    expect(
      render(
        "a",
        <Inspect node="a" slot="head">
          Name
        </Inspect>,
      ),
    ).toBe("");
  });
});
