import { describe, expect, it } from "vitest";
import type { EncoderDescriptor, PatchNode } from "../../lib/types";
import { renderFace } from "../../test/faceHarness";
import { EncoderFace } from "./EncoderFace";

const NFM: EncoderDescriptor = {
  type_id: "nfm",
  name: "NFM",
  rate_hz: 48_000,
  bandwidth_hz: 12_500,
  input: "audio",
  settings: [
    {
      key: "bandwidth_hz",
      label: "BW",
      control: { type: "number", unit: "Hz" },
      fallback: 12_500,
    },
  ],
  defaults: { bandwidth_hz: 12_500 },
};

const NODE = {
  id: "tx",
  kind: "encoder",
  position: { x: 0, y: 0 },
  data: { encoder_type: "nfm" },
} as PatchNode;

function face(encoderTypes: readonly EncoderDescriptor[] = [NFM]): string {
  return renderFace(EncoderFace, NODE, {
    graph: { nodes: [NODE], edges: [] },
    resolved: null,
    encoderTypes,
  });
}

describe("EncoderFace", () => {
  it("asks for audio and shows its own settings", () => {
    const html = face();
    expect(html).toContain("Wire audio in");
    expect(html).toContain("BW");
    expect(html).not.toContain("Mode");
  });

  it("names a mode no encoder brings", () => {
    const html = face([]);
    expect(html).toContain("Mode not loaded");
    expect(html).toContain("nfm");
  });
});
