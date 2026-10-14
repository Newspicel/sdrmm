import { afterEach, describe, expect, it } from "vitest";
import {
  clearAction,
  flagAction,
  MAX_REFUSALS,
  NOT_CONNECTED,
  newestRefusal,
  useRefusalStore,
} from "./refusals";

afterEach(() => useRefusalStore.getState().reset());

function listOf(node: string) {
  return useRefusalStore.getState().byNode[node];
}

describe("useRefusalStore", () => {
  it("replaces apply refusals with each report", () => {
    const store = useRefusalStore.getState();
    store.fromReport({
      bound: [],
      created: 0,
      opened: 0,
      refused: [{ node: "df", reason: "array has no lanes" }],
      absent: ["dev"],
    });
    expect(listOf("df")?.map((refusal) => refusal.reason)).toEqual(["array has no lanes"]);
    expect(listOf("dev")?.map((refusal) => refusal.reason)).toEqual([NOT_CONNECTED]);
    store.flag("df", "Calibrate failed", "action");
    store.fromReport({ bound: [], created: 0, opened: 0 });
    expect(listOf("dev")).toBeUndefined();
    expect(listOf("df")?.map((refusal) => refusal.source)).toEqual(["action"]);
  });

  it("drops radio not connected once the node has its radio", () => {
    const store = useRefusalStore.getState();
    store.fromReport({
      bound: [],
      created: 0,
      opened: 0,
      refused: [{ node: "df", reason: "array has no lanes" }],
      absent: ["dev"],
    });
    store.connected(["dev", "df"]);
    expect(listOf("dev")).toBeUndefined();
    expect(listOf("df")?.map((refusal) => refusal.reason)).toEqual(["array has no lanes"]);
  });

  it("keeps action refusals until dismissed", () => {
    const store = useRefusalStore.getState();
    store.flag("arr", "Calibrate failed", "action");
    expect(newestRefusal(listOf("arr"))?.reason).toBe("Calibrate failed");
    store.dismiss("arr");
    expect(listOf("arr")).toBeUndefined();
  });

  it("shows the newest refusal", () => {
    const store = useRefusalStore.getState();
    store.flag("arr", "first", "action");
    store.flag("arr", "second", "apply");
    expect(newestRefusal(listOf("arr"))?.reason).toBe("second");
  });

  it("clears only the refusal of the action that later succeeded", () => {
    const store = useRefusalStore.getState();
    flagAction("arr", "Tune", new Error("Held"));
    flagAction("arr", "Calibrate", new Error("Busy"));
    store.flag("arr", "radio not connected", "apply");
    expect(listOf("arr")?.map((refusal) => refusal.reason)).toEqual([
      "Tune: Held",
      "Calibrate: Busy",
      "radio not connected",
    ]);
    clearAction("arr", "Tune");
    expect(listOf("arr")?.map((refusal) => refusal.reason)).toEqual([
      "Calibrate: Busy",
      "radio not connected",
    ]);
    clearAction("arr", "Calibrate");
    clearAction("other", "Calibrate");
    expect(listOf("arr")?.map((refusal) => refusal.source)).toEqual(["apply"]);
  });

  it("keeps a short list", () => {
    const store = useRefusalStore.getState();
    for (let index = 0; index < MAX_REFUSALS + 3; index++) {
      store.flag("arr", `failed ${index}`, "action");
    }
    expect(listOf("arr")).toHaveLength(MAX_REFUSALS);
    expect(listOf("arr")?.at(-1)?.reason).toBe(`failed ${MAX_REFUSALS + 2}`);
  });
});
