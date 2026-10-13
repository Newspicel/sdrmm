import { afterEach, describe, expect, it, vi } from "vitest";
import { wiringVerdicts } from "../lib/api";
import type { PatchGraph, WireVerdict } from "../lib/types";
import { NOT_OFFERED, useWiringStore } from "../lib/wiring";
import { CATALOG, catalogBody } from "../test/catalog";
import { placed } from "../test/fixtures";
import type { GraphContext } from "./graph";
import { askVerdicts, refusalOf } from "./handlers";

vi.mock(import("../lib/api"), async (original) => ({
  ...(await original()),
  wiringVerdicts: vi.fn(),
}));

const GRAPH: PatchGraph = {
  nodes: [
    placed("dev", { kind: "device", data: {} }),
    placed("arr", catalogBody("array")),
    placed("scope", { kind: "scope" }),
    placed("spk", { kind: "speaker" }),
  ],
  edges: [],
};

const CONTEXT: GraphContext = { catalog: CATALOG, channelTypes: [] };
const FROM = { node: "dev", port: "iq" };
const LANE = { node: "arr", port: "lane" };
const SCOPE = { node: "scope", port: "iq" };

function deferred(): {
  promise: Promise<WireVerdict[]>;
  resolve: (verdicts: WireVerdict[]) => void;
} {
  let settle: ((verdicts: WireVerdict[]) => void) | undefined;
  const promise = new Promise<WireVerdict[]>((resolve) => {
    settle = resolve;
  });
  return { promise, resolve: (verdicts) => settle?.(verdicts) };
}

afterEach(() => {
  useWiringStore.getState().clear();
  vi.mocked(wiringVerdicts).mockReset();
});

describe("dragging a wire", () => {
  it("lights same-type inputs until the server answers, then follows its verdicts", async () => {
    const pending = deferred();
    vi.mocked(wiringVerdicts).mockReturnValue(pending.promise);
    const asked = askVerdicts(GRAPH, FROM);
    expect(wiringVerdicts).toHaveBeenCalledWith(GRAPH, FROM);
    expect(refusalOf(CONTEXT, GRAPH, FROM, LANE)).toBeNull();
    expect(refusalOf(CONTEXT, GRAPH, FROM, { node: "spk", port: "audio" })).toBe(
      "iq cannot feed audio",
    );
    pending.resolve([{ to: LANE, refusal: "That lane is in North" }, { to: SCOPE }]);
    await asked;
    expect(refusalOf(CONTEXT, GRAPH, FROM, LANE)).toBe("That lane is in North");
    expect(refusalOf(CONTEXT, GRAPH, FROM, SCOPE)).toBeNull();
  });

  it("asks nothing for a drag that starts on an input", async () => {
    await askVerdicts(GRAPH, null);
    expect(wiringVerdicts).not.toHaveBeenCalled();
    expect(useWiringStore.getState().verdicts).toBeNull();
  });

  it("refuses an input the server did not list", async () => {
    vi.mocked(wiringVerdicts).mockResolvedValue([{ to: SCOPE }]);
    await askVerdicts(GRAPH, FROM);
    expect(refusalOf(CONTEXT, GRAPH, FROM, LANE)).toBe(NOT_OFFERED);
  });
});
