import { afterEach, describe, expect, it } from "vitest";
import { useArrayStore } from "../../lib/arrays";
import { useProcessorStore } from "../../lib/processors";
import type {
  ArrayStatus,
  Field,
  NodeKind,
  PatchCatalog,
  PatchGraph,
  PatchNode,
  ProcessorStatus,
  ProcessorType,
  ProcessorView,
} from "../../lib/types";
import { CATALOG, catalogBody } from "../../test/catalog";
import { renderFace } from "../../test/faceHarness";
import { arrayStatus, laneStatus, placed } from "../../test/fixtures";
import { ArrayProcessorFace as ProcessorFace } from "./ArrayProcessorFace";
import { NOT_LOADED } from "./processorFace";
import { NEEDS_HEADING } from "./views/bearing";

const FIVE: ArrayStatus = arrayStatus("arr", {
  lanes: [0, 1, 2, 3, 4].map((lane) => laneStatus(lane)),
});

function wired(kind: NodeKind, extra: PatchNode[] = []): { node: PatchNode; graph: PatchGraph } {
  const node = placed("proc", catalogBody(kind));
  return {
    node,
    graph: {
      nodes: [placed("arr", catalogBody("array")), ...extra, node],
      edges: [{ from: { node: "arr", port: "array" }, to: { node: "proc", port: "array" } }],
    },
  };
}

function text(key: string, label: string, value: string): Field {
  return { key, label, value: { type: "text", value } };
}

function holding(view: ProcessorView | null, readouts: Field[] = [], status = FIVE): void {
  useArrayStore.getState().seed([status]);
  useProcessorStore.setState({
    byNode:
      view === null
        ? {}
        : { proc: { view, status: { readouts }, receivedAt: Date.now(), periodMs: 0 } },
  });
}

function processor(overrides: Partial<ProcessorStatus> = {}): ProcessorStatus {
  return {
    node: "proc",
    kind: "df",
    running: true,
    gated_samples: 0,
    dropped_samples: 0,
    dropped_reports: 0,
    lane_overflows: 0,
    lane_mismatch: 0,
    solver_failures: 0,
    resets: 0,
    ...overrides,
  };
}

function attributesOf(html: string, label: string): string {
  return new RegExp(`<(\\w+)([^<>]*)>${label}</\\1>`).exec(html)?.[2] ?? "";
}

function pressed(html: string, label: string): boolean {
  return attributesOf(html, label).includes('aria-pressed="true"');
}

function chip(html: string, label: string): string | undefined {
  return new RegExp(`>${label}</span><b[^>]*>([^<]*)<`).exec(html)?.[1];
}

function plain(html: string): string {
  return html.replace(/<!-- -->/g, "").replace(/<[^>]+>/g, "");
}

afterEach(() => {
  useProcessorStore.getState().reset();
  useArrayStore.getState().reset();
});

const BEARING: ProcessorView = {
  bearing: {
    spectrum: Array.from({ length: 360 }, (_, deg) => (deg === 47 ? 255 : 20)),
    heading_deg: 90,
    needles: [{ relative_deg: 47, true_deg: 137, weight: "primary" }],
    sigma_deg: 3.1,
  },
};

describe("a bearing face", () => {
  it("shows the true frame once heading is known", () => {
    holding(BEARING, [text("bearing", "Bearing", "137°")]);
    const { node, graph } = wired("df");
    const html = renderFace(ProcessorFace, node, { graph });
    expect(pressed(html, "True")).toBe(true);
    expect(html).toContain('aria-label="Bearing rose"');
    expect(html).toContain(">N</text>");
    expect(html).toMatch(/>Bearing<.*>137°</);
    expect(html).toMatch(/>Method<\/span><b[^>]*>MUSIC</);
    expect(html).toContain("5 lanes");
    expect(html).toContain(">Direction finder<");
  });

  it("disables True without heading and lists the reading's flags", () => {
    holding({
      bearing: { ...BEARING.bearing, spectrum: [], needles: [], heading_deg: null },
      flags: [{ label: "No heading", title: "Wire a GPS with heading" }],
    });
    const { node, graph } = wired("df");
    const html = renderFace(ProcessorFace, node, { graph });
    expect(pressed(html, "Rel")).toBe(true);
    expect(html).toContain(`title="${NEEDS_HEADING}"`);
    expect(html).toContain(">F</text>");
    expect(html).toMatch(/title="Wire a GPS with heading"[^>]*>No heading</);
  });

  it("explains a gated processor and says when no array is wired", () => {
    holding(BEARING, [], { ...FIVE, processors: [processor({ gated: "phase" })] });
    const { node, graph } = wired("df");
    expect(renderFace(ProcessorFace, node, { graph })).toContain("needs cal");
    expect(renderFace(ProcessorFace, node, { graph: { nodes: graph.nodes, edges: [] } })).toContain(
      "no array",
    );
  });

  it("shows what the processor lost and why it stopped", () => {
    const failing = processor({ dropped_reports: 3, truncated: 2, error: "Needs a circle" });
    holding(BEARING, [], { ...FIVE, processors: [failing] });
    const { node, graph } = wired("df");
    const html = renderFace(ProcessorFace, node, { graph });
    expect(html).toMatch(/>Lost<.*>3</);
    expect(html).toMatch(/>Cut<.*>2</);
    expect(html).toMatch(/role="alert"[^>]*><p[^>]*>Needs a circle</);
  });
});

describe("a pattern face", () => {
  it("draws the pattern, the lane weights and the mode's form", () => {
    holding({
      pattern: {
        pattern: Array.from({ length: 360 }, () => 128),
        steer_deg: 137,
        nulls_deg: [45],
        weights: [
          { amplitude_db: 0, phase_deg: 0 },
          { amplitude_db: -1.2, phase_deg: 34 },
        ],
      },
      flags: [{ label: "No steer", title: "No bearing to steer at" }],
    });
    const { node, graph } = wired("beamformer");
    const html = renderFace(ProcessorFace, node, { graph });
    expect(html).toContain('aria-label="Beam pattern"');
    expect(html).toContain('title="L2 -1.2 dB 34°"');
    expect(html).toContain('aria-valuenow="87"');
    expect(chip(html, "Mode")).toBeDefined();
    expect(html).toMatch(/title="No bearing to steer at"[^>]*>No steer</);
  });
});

describe("a surface face", () => {
  const RADAR: ProcessorView = {
    headline: { text: "98.00 MHz" },
    flags: [{ label: "Outside cal table", title: "Bearings use the ideal geometry" }],
    surface: {
      marks: [{ x: 42.1, y: 40, shape: "square", key: 3, label: "T3" }],
    },
    table: {
      heads: ["Track", "km", "m/s", "Bearing", "dB", "Truth"],
      rows: [
        {
          key: 3,
          pick: true,
          title: "Show on the plot",
          cells: [
            { text: "T3" },
            { text: "42.1" },
            { text: "-120" },
            { text: "231°", faint: true, title: "Array frame" },
            { text: "14" },
            { text: "DLH4AB" },
          ],
        },
      ],
    },
  };

  it("labels the plot, lists the rows and offers the declared action", () => {
    holding(RADAR);
    const { node, graph } = wired("passive_radar");
    const html = renderFace(ProcessorFace, node, { graph });
    for (const unit of [">km<", ">Hz<", ">m/s<", ">dB<"]) {
      expect(html).toContain(unit);
    }
    expect(html).toContain("DLH4AB");
    expect(html).toContain('title="Array frame"');
    expect(html).toContain("98.00 MHz");
    expect(html).toMatch(/title="Forget every track"[^>]*>Clear</);
    expect(html).toMatch(/>Colours</);
    expect(html.split("Outside cal table")).toHaveLength(2);
  });

  it("warns with the reading's headline", () => {
    holding({ headline: { text: "No transmitter", warn: true } });
    const { node, graph } = wired("passive_radar");
    expect(renderFace(ProcessorFace, node, { graph })).toMatch(/class="text-warn">No transmitter</);
  });

  it("offers a frame toggle over a bearing surface", () => {
    holding({ surface: { heading_deg: 90, marks: [] } }, [text("peak", "Peak", "433.920 MHz")]);
    const { node, graph } = wired("spatial_spectrum");
    const html = renderFace(ProcessorFace, node, { graph });
    expect(plain(html)).toContain("433.920 MHz");
    expect(html).toContain('aria-label="Bearing frame"');
    expect(html).toMatch(/>Columns<\/span><b[^>]*>256</);
  });

  it("sums up the shown layer", () => {
    holding({
      surface: {
        layers: [
          { label: "1-2", text: "0.87 ∠37° 1.2 ns 18 dB", title: "x", phase_deg: 37 },
          { label: "1-3", text: "", title: "x" },
        ],
      },
    });
    const { node, graph } = wired("correlator");
    const html = plain(renderFace(ProcessorFace, node, { graph }));
    expect(html).toContain("Baseline");
    expect(html).toContain("0.87 ∠37° 1.2 ns 18 dB");
  });
});

describe("other declared views", () => {
  it("draws the polarisation ellipse beside its lane form", () => {
    holding({ ellipse: { angle_deg: 23, ellipticity_deg: -5, v: 0.5 } }, [
      text("hand", "Hand", "RH"),
    ]);
    const { node, graph } = wired("polarimeter");
    const html = renderFace(ProcessorFace, node, { graph });
    expect(html).toContain('aria-label="Polarisation"');
    expect(html).toContain("RH");
    expect(html).toMatch(/>H<\/span><b[^>]*>1</);
  });

  it("asks for the array tuning the processor declares", () => {
    holding({
      table: { heads: ["Lane"], rows: [{ key: 0, cells: [{ text: "L1" }] }] },
    });
    const { node, graph } = wired("stitch");
    const html = renderFace(ProcessorFace, node, { graph });
    expect(html).toContain(">Needs spread<");
    expect(html).toMatch(/title="Tune the wired array[^"]*"[^>]*>Spread array</);
    expect(html).toContain(">L1<");
  });
});

const METER: ProcessorType = {
  type_id: "lane_meter",
  name: "Lane meter",
  summary: "Power and phase of each lane",
  settings: [
    { key: "blocks", label: "Average", control: { type: "number", min: 1, max: 256, step: 1 } },
  ],
  defaults: { blocks: 8 },
};

const WITH_METER: PatchCatalog = { ...CATALOG, processors: [METER] };

function pluginNode(type: string): PatchNode {
  return placed("proc", { kind: "processor", data: { processor_type: type, settings: null } });
}

describe("a plugin processor", () => {
  it("gets the same face from its type", () => {
    holding({}, [text("power_1", "P1", "-6.0 dBFS")]);
    const node = pluginNode("lane_meter");
    const html = plain(
      renderFace(ProcessorFace, node, {
        graph: { nodes: [node], edges: [] },
        catalog: WITH_METER,
        resolved: null,
      }),
    );
    expect(html).toContain("Lane meter");
    expect(html).toContain("-6.0 dBFS");
    expect(html).toContain("Average");
    expect(html).not.toContain(NOT_LOADED);
  });

  it("says when no plugin brings its type", () => {
    const node = pluginNode("gone_meter");
    const html = plain(renderFace(ProcessorFace, node, { graph: { nodes: [node], edges: [] } }));
    expect(html).toContain("gone_meter");
    expect(html).toContain(NOT_LOADED);
  });
});
