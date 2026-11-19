import { describe, expect, it } from "vitest";
import type { PatchNode } from "../../lib/types";
import { renderFace } from "../../test/faceHarness";
import { placed } from "../../test/fixtures";
import { bandLabel, SpectrumMonitorFace } from "./SpectrumMonitorFace";

function render(data: { start_hz?: number; stop_hz?: number }, inspected = true): string {
  const node: PatchNode = placed("monitor", { kind: "spectrum_monitor", data });
  return renderFace(SpectrumMonitorFace, node, { graph: { nodes: [node], edges: [] }, inspected });
}

describe("SpectrumMonitorFace", () => {
  it("shows all until a range is set", () => {
    const html = render({});
    expect(html.match(/placeholder="all"[^>]*value=""/g)).toHaveLength(2);
  });

  it("shows the range in MHz", () => {
    const html = render({ start_hz: 446e6, stop_hz: 446.2e6 });
    expect(html).toContain('value="446"');
    expect(html).toContain('value="446.2"');
  });

  it("keeps its settings off the face", () => {
    expect(render({}, false)).not.toContain("Lowest frequency");
  });

  it("reads its settings on the face", () => {
    const html = render({ start_hz: 446e6, stop_hz: 446.2e6 }, false);
    expect(html).toContain("Band");
    expect(html).toContain("446–446.2");
    expect(html).toContain("Protocols");
  });
});

describe("bandLabel", () => {
  it("reads open edges", () => {
    expect(bandLabel(null, null)).toBe("All");
    expect(bandLabel(446, null)).toBe("446–… MHz");
    expect(bandLabel(null, 450)).toBe("…–450 MHz");
  });
});
