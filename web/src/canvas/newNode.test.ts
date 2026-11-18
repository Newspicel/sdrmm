import { describe, expect, it } from "vitest";
import type { PatchCatalog, PatchNodeOf } from "../lib/types";
import { CATALOG } from "../test/catalog";
import { carriesSettings, defaultBody, huntSweepOf, newNodeBody } from "./newNode";
import { nodeSettings } from "./nodes/NodeForm";

const KINDS = CATALOG.nodes
  .filter((entry) => entry.picks !== "processor")
  .map((entry) => entry.kind);

describe("newNodeBody", () => {
  it("takes every body from the catalog", () => {
    for (const entry of CATALOG.nodes) {
      if (entry.picks != null) {
        continue;
      }
      expect(newNodeBody(CATALOG, entry.kind)).toEqual(entry.default_body);
    }
  });

  it("hands out a fresh copy each time", () => {
    const listed = CATALOG.nodes.find((entry) => entry.kind === "triangulation")?.default_body;
    const first = newNodeBody(CATALOG, "triangulation");
    expect(first).toEqual(listed);
    expect(first).not.toBe(listed);
    expect(newNodeBody(CATALOG, "triangulation")).not.toBe(first);
  });

  it("returns null for an unknown kind", () => {
    const empty: PatchCatalog = { nodes: [], port_types: [], event_kinds: [] };
    expect(newNodeBody(empty, "array")).toBeNull();
    expect(defaultBody(empty, "scope")).toBeNull();
  });

  it("starts a channel of the picked type", () => {
    expect(newNodeBody(CATALOG, "channel", { typeId: "probe" })).toEqual({
      kind: "channel",
      data: { channel_type: "probe" },
    });
    expect(newNodeBody(CATALOG, "channel")).toBeNull();
  });

  it("starts an encoder of the picked mode", () => {
    expect(newNodeBody(CATALOG, "encoder", { typeId: "nfm" })).toEqual({
      kind: "encoder",
      data: { encoder_type: "nfm" },
    });
    expect(newNodeBody(CATALOG, "encoder")).toBeNull();
  });

  it.each(KINDS)("gives %s a body of its own kind", (kind) => {
    const body = newNodeBody(CATALOG, kind, { typeId: "probe" });
    expect(body?.kind).toBe(kind);
    if (carriesSettings(CATALOG, kind)) {
      expect((body as { data?: unknown }).data).toBeDefined();
    }
  });

  it("knows which kinds carry no settings", () => {
    expect(carriesSettings(CATALOG, "scope")).toBe(false);
    expect(carriesSettings(CATALOG, "triangulation")).toBe(true);
    expect(carriesSettings({ nodes: [], port_types: [], event_kinds: [] }, "triangulation")).toBe(
      false,
    );
  });
});

describe("nodeSettings", () => {
  it("reads stored settings and falls back to the catalog default", () => {
    const stored = newNodeBody(CATALOG, "processor", { typeId: "df" }) as PatchNodeOf<"processor">;
    const node = {
      ...stored,
      id: "processor:1",
      position: { x: 0, y: 0 },
      data: { ...stored.data, settings: { ...(stored.data.settings as object), report_ms: 900 } },
    } as PatchNodeOf<"processor">;
    expect(nodeSettings(CATALOG, node)?.report_ms).toBe(900);
    const bare = { ...node, data: { processor_type: "df" } } as PatchNodeOf<"processor">;
    expect(nodeSettings(CATALOG, bare)).toEqual(stored.data.settings);
  });
});

describe("huntSweepOf", () => {
  it("reads the hunt's own sweep and falls back to the catalog default", () => {
    const walk = {
      id: "hunt:1",
      position: { x: 0, y: 0 },
      kind: "hunt",
      data: {},
    } as PatchNodeOf<"hunt">;
    const fallback = huntSweepOf(walk, CATALOG);
    expect(fallback?.beamwidth_deg).toBe(60);
    const mounted = { ...walk, data: { sweep: { ...fallback, mount_offset_deg: -90 } } };
    expect(huntSweepOf(mounted as PatchNodeOf<"hunt">, CATALOG)?.mount_offset_deg).toBe(-90);
    expect(huntSweepOf(walk, { nodes: [], port_types: [], event_kinds: [] })).toBeNull();
  });
});
