import { afterEach, describe, expect, it } from "vitest";
import { useProcessorStore } from "../../lib/processors";
import type { BeamformerParams, NodeBody, PatchGraph, PatchNode } from "../../lib/types";
import { catalogBody } from "../../test/catalog";
import { renderFace } from "../../test/faceHarness";
import { placed } from "../../test/fixtures";
import { BeamformerFace } from "./BeamformerFace";

function beam(settings: Partial<BeamformerParams>): PatchNode {
  const body = catalogBody("beamformer");
  if (body.kind !== "beamformer" || body.data.settings === undefined) {
    throw new Error("a beamformer body");
  }
  const tuned: NodeBody = { ...body, data: { settings: { ...body.data.settings, ...settings } } };
  return placed("beam", tuned);
}

const BEAM = beam({ mode: "lcmv", nulls_deg: [45] });

const GRAPH: PatchGraph = {
  nodes: [placed("arr", catalogBody("array")), placed("finder", catalogBody("df")), BEAM],
  edges: [
    { from: { node: "arr", port: "array" }, to: { node: "beam", port: "array" } },
    { from: { node: "finder", port: "bearing" }, to: { node: "beam", port: "steer" } },
  ],
};

function chip(html: string, label: string): string | undefined {
  return new RegExp(`>${label}</span><b[^>]*>([^<]*)<`).exec(html)?.[1];
}

afterEach(() => useProcessorStore.getState().reset());

describe("BeamformerFace", () => {
  it("draws the settings its mode uses from the catalog form", () => {
    const html = renderFace(BeamformerFace, BEAM, { graph: GRAPH });
    expect(chip(html, "Mode")).toBe("Nulls");
    expect(chip(html, "Nulls")).toBe("45°");
    expect(chip(html, "Steer")).toBe("DF");
    expect(chip(html, "Timeout")).toBe("5000");
    expect(html).not.toContain(">Main<");
    expect(html).not.toContain(">Noise<");
  });

  it("offers lanes to a canceller and names an empty reference set", () => {
    const canceller = beam({ mode: "canceller", taps: 4 });
    const html = renderFace(BeamformerFace, canceller, { graph: GRAPH, resolved: null });
    expect(chip(html, "Main")).toBe("1");
    expect(chip(html, "Refs")).toBe("others");
    expect(chip(html, "Adapt")).toBe("NLMS");
    expect(html).toContain(">Step<");
    expect(html).not.toContain(">Forget<");
    expect(html).not.toContain(">Steer<");
  });

  it("shows the readouts its status publishes and the weights it drew", () => {
    useProcessorStore.setState({
      byNode: {
        beam: {
          reading: {
            type: "beamformer",
            reading: {
              at: "2026-09-28T12:00:00Z",
              output_db: -42,
              steer_deg: 137,
              nulls_deg: [45],
              weights: [
                { amplitude_db: 0, phase_deg: 0 },
                { amplitude_db: -1.2, phase_deg: 34 },
              ],
              pattern: Array.from({ length: 360 }, () => 128),
            },
          },
          status: {
            readouts: [
              { key: "gain", label: "Gain", value: { type: "text", value: "+6.2 dB" } },
              {
                key: "steer",
                label: "Steer",
                value: { type: "number", value: { value: 137, unit: "degrees", digits: 0 } },
              },
            ],
          },
          receivedAt: Date.now(),
        },
      },
    });
    const html = renderFace(BeamformerFace, BEAM, { graph: GRAPH });
    expect(html).toMatch(/>Gain<.*>\+6\.2 <span[^>]*>dB</);
    expect(html).toContain("137°");
    expect(html).toContain('title="L2 -1.2 dB 34°"');
    expect(html).toContain('role="meter" aria-label="Lane 2 weight"');
    expect(html).toContain('aria-valuenow="87"');
    expect(html).toContain('aria-label="Beam pattern"');
  });

  it("puts reading flags in the footer", () => {
    useProcessorStore.setState({
      byNode: {
        beam: {
          reading: {
            type: "beamformer",
            reading: {
              at: "2026-09-28T12:00:00Z",
              output_db: -42,
              nulls_deg: [],
              weights: [],
              pattern: [],
              no_steer: true,
            },
          },
          receivedAt: Date.now(),
        },
      },
    });
    const html = renderFace(BeamformerFace, BEAM, { graph: GRAPH });
    expect(html).toMatch(/title="No bearing to steer at"[^>]*>No steer</);
  });
});
