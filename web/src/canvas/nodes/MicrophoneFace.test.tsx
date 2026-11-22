import { describe, expect, it } from "vitest";
import type { PatchNode } from "../../lib/types";
import { renderFace } from "../../test/faceHarness";
import { MicrophoneFace } from "./MicrophoneFace";

function microphone(send: "push_to_talk" | "voice_activity"): PatchNode {
  return {
    id: "mic",
    kind: "microphone",
    position: { x: 0, y: 0 },
    data: { send, threshold_db: -35, hang_ms: 600 },
  };
}

function render(node: PatchNode): string {
  return renderFace(MicrophoneFace, node, {
    graph: { nodes: [node], edges: [] },
    resolved: null,
    inspected: true,
  });
}

describe("MicrophoneFace", () => {
  it("picks how it sends from a dropdown, with no voice key switch", () => {
    const html = render(microphone("voice_activity"));
    expect(html).toContain('aria-label="Microphone: when audio is sent"');
    expect(html).toContain(">Voice activity<");
    expect(html).toContain('aria-label="Microphone voice level"');
    expect(html).not.toContain("VOX");
  });

  it("holds the talk key back while no armed transmitter hears it", () => {
    const html = render(microphone("push_to_talk"));
    expect(html).toContain('title="No armed transmitter"');
    expect(html).toMatch(/<button[^>]*disabled[^>]*>Talk</);
  });
});
