import { describe, expect, it } from "vitest";
import type { PatchNode } from "../../lib/types";
import { renderFace } from "../../test/faceHarness";
import { placed } from "../../test/fixtures";
import { SpectrumMonitorFace } from "./SpectrumMonitorFace";

function render(data: { start_hz?: number; stop_hz?: number }): string {
  const node: PatchNode = placed("monitor", { kind: "spectrum_monitor", data });
  return renderFace(SpectrumMonitorFace, node, { graph: { nodes: [node], edges: [] } });
}

describe("SpectrumMonitorFace", () => {
  it("shows all until a range is set", () => {
    expect(render({}).match(/>all</g)).toHaveLength(2);
  });

  it("shows the range in MHz", () => {
    const html = render({ start_hz: 446e6, stop_hz: 446.2e6 });
    expect(html).toContain(">446<");
    expect(html).toContain(">446.2<");
    expect(html).not.toContain(">all<");
  });
});
