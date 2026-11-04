import { afterEach, describe, expect, it } from "vitest";
import { useArrayStore } from "../../lib/arrays";
import { useProcessorStore } from "../../lib/processors";
import type { PatchCatalog, PatchGraph, PatchNode, ProcessorType } from "../../lib/types";
import { CATALOG } from "../../test/catalog";
import { renderFace } from "../../test/faceHarness";
import { placed } from "../../test/fixtures";
import { newNodeBody } from "../newNode";
import { paletteGroups } from "../palette";
import { formOf, nodeSettings } from "./NodeForm";
import { NOT_LOADED, PluginProcessorFace } from "./PluginProcessorFace";

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

function processorNode(type: string, settings: unknown = null): PatchNode {
  return placed("proc", { kind: "processor", data: { processor_type: type, settings } });
}

function graphOf(node: PatchNode): PatchGraph {
  return { nodes: [node], edges: [] };
}

function textOf(html: string): string {
  return html.replace(/<!-- -->/g, "").replace(/<[^>]+>/g, "");
}

afterEach(() => {
  useProcessorStore.getState().reset();
  useArrayStore.getState().reset();
});

describe("plugin processor face", () => {
  it("draws the plugin's form and readouts with no face of its own", () => {
    useProcessorStore.setState({
      byNode: {
        proc: {
          reading: { type: "plugin", reading: { type_id: "lane_meter", readouts: [] } },
          status: {
            readouts: [
              { key: "power_1", label: "P1", value: { type: "text", value: "-6.0 dBFS" } },
            ],
          },
          receivedAt: Date.now(),
        },
      },
    });
    const node = processorNode("lane_meter");
    const html = renderFace(PluginProcessorFace, node, {
      graph: graphOf(node),
      catalog: WITH_METER,
      resolved: null,
    });
    const text = textOf(html);
    expect(text).toContain("Lane meter");
    expect(text).toContain("-6.0 dBFS");
    expect(text).toContain("Average");
    expect(text).not.toContain(NOT_LOADED);
  });

  it("renders a type no plugin brings", () => {
    const node = processorNode("gone_meter", { blocks: 3 });
    const text = textOf(
      renderFace(PluginProcessorFace, node, {
        graph: graphOf(node),
        catalog: WITH_METER,
        resolved: null,
      }),
    );
    expect(text).toContain("gone_meter");
    expect(text).toContain(NOT_LOADED);
  });

  it("starts a fresh node on its type's defaults and form", () => {
    const body = newNodeBody(WITH_METER, "processor", { processorType: "lane_meter" });
    expect(body).toEqual({
      kind: "processor",
      data: { processor_type: "lane_meter", settings: { blocks: 8 } },
    });
    const node = processorNode("lane_meter");
    expect(nodeSettings(WITH_METER, node)).toEqual({ blocks: 8 });
    expect(formOf(WITH_METER, node).map((field) => field.key)).toEqual(["blocks"]);
    expect(newNodeBody(WITH_METER, "processor", { processorType: "gone" })).toBeNull();
  });

  it("offers each loaded processor in the palette, not the bare kind", () => {
    const items = paletteGroups(WITH_METER, []).flatMap((group) => group.items);
    expect(items.filter((item) => item.kind === "processor")).toEqual([
      expect.objectContaining({ id: "processor:lane_meter", processorType: "lane_meter" }),
    ]);
    const bare = paletteGroups(CATALOG, []).flatMap((group) => group.items);
    expect(bare.some((item) => item.kind === "processor")).toBe(false);
  });
});
