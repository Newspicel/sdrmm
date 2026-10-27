import { describe, expect, it } from "vitest";
import type { ExampleInfo, ExampleNeeds, NodeKind, PatchCatalog } from "../lib/types";
import { exampleNeeds, examplesHint, kindLabels } from "./examples";

function example(needs: Partial<ExampleNeeds>, node_kinds: NodeKind[] = []): ExampleInfo {
  return {
    id: "x",
    name: "X",
    description: "",
    node_kinds,
    needs: { radios: 0, lanes: 0, recordings: 0, min_freq_hz: 0, max_freq_hz: 0, ...needs },
  };
}

describe("exampleNeeds", () => {
  it("says nothing is needed when there are no sources", () => {
    expect(exampleNeeds(example({}))).toBe("no radio");
  });

  it("counts radios", () => {
    expect(exampleNeeds(example({ radios: 1 }))).toBe("1 radio");
  });

  it("counts more than one radio", () => {
    expect(exampleNeeds(example({ radios: 2 }))).toBe("2 radios");
  });

  it("prefers lanes when the example is an array", () => {
    expect(exampleNeeds(example({ radios: 5, lanes: 5 }))).toBe("5 lanes");
  });

  it("counts recordings", () => {
    expect(exampleNeeds(example({ recordings: 1 }))).toBe("1 recording");
  });
});

describe("examplesHint", () => {
  it("says nothing once the list has loaded", () => {
    expect(examplesHint(false, null)).toBeNull();
  });

  it("says it is loading while the query is pending", () => {
    expect(examplesHint(true, null)).toBe("Loading examples.");
  });

  it("surfaces the server's reason rather than showing an empty tab", () => {
    expect(examplesHint(false, new Error("the no-radio example is malformed: bad gzip"))).toBe(
      "the no-radio example is malformed: bad gzip",
    );
  });

  it("falls back to a message when the error carries none", () => {
    expect(examplesHint(false, new Error(""))).toBe("The examples could not be read.");
  });
});

describe("kindLabels", () => {
  const catalog = {
    nodes: [{ kind: "device", name: "Device" }],
  } as unknown as PatchCatalog;

  it("names a kind the catalog knows", () => {
    expect(kindLabels(example({}, ["device"]), catalog)).toEqual(["Device"]);
  });

  it("falls back to the raw kind when the catalog has not loaded", () => {
    expect(kindLabels(example({}, ["device"]), undefined)).toEqual(["device"]);
  });

  it("falls back to the raw kind the catalog does not list", () => {
    expect(kindLabels(example({}, ["note"]), catalog)).toEqual(["note"]);
  });
});
