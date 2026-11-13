import { describe, expect, it } from "vitest";
import type { PatchCatalog, PatchNode, ProcessorType } from "../lib/types";
import { CATALOG } from "../test/catalog";
import { placed } from "../test/fixtures";
import { newNodeBody } from "./newNode";
import { formOf, nodeSettings } from "./nodes/NodeForm";
import { paletteGroups } from "./palette";

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

function processorNode(type: string): PatchNode {
  return placed("proc", { kind: "processor", data: { processor_type: type, settings: null } });
}

describe("plugin processor types", () => {
  it("start a fresh node on their defaults and form", () => {
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

  it("each show in the palette, not the bare kind", () => {
    const items = paletteGroups(WITH_METER, []).flatMap((group) => group.items);
    expect(items.filter((item) => item.kind === "processor")).toEqual([
      expect.objectContaining({ id: "processor:lane_meter", processorType: "lane_meter" }),
    ]);
    const bare = paletteGroups(CATALOG, []).flatMap((group) => group.items);
    expect(bare.some((item) => item.kind === "processor")).toBe(false);
  });
});
