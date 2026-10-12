import { afterEach, describe, expect, it } from "vitest";
import type { PortSpec } from "./types";
import { NO_PORT, NOT_OFFERED, SELF_WIRE, useWiringStore, wireRefusal } from "./wiring";

const iq = (name: string, direction: "in" | "out"): PortSpec => ({
  name,
  port_type: "iq",
  direction,
  multi: direction === "out" ? "many" : "one",
  payload: { type: "iq", origins: ["radio"] },
});

const audioIn: PortSpec = {
  name: "audio",
  port_type: "audio",
  direction: "in",
  multi: "many",
  payload: { type: "audio", streams: ["live"] },
};

const from = { node: "dev", port: "iq2" };
const lane = { node: "arr", port: "lane2" };

afterEach(() => useWiringStore.getState().clear());

describe("wire verdicts", () => {
  it("lights every input of the dragged type until the server answers", () => {
    const ends = { from, to: lane, out: iq("iq2", "out"), input: iq("lane2", "in") };
    expect(wireRefusal(null, ends)).toBeNull();
    expect(wireRefusal(null, { ...ends, to: { node: "spk", port: "audio" }, input: audioIn })).toBe(
      "iq cannot feed audio",
    );
    expect(wireRefusal(null, { ...ends, input: undefined })).toBe(NO_PORT);
    expect(wireRefusal(null, { ...ends, to: { node: "dev", port: "x" } })).toBe(SELF_WIRE);
  });

  it("takes the server's verdict once it arrives", () => {
    const store = useWiringStore.getState();
    store.start(from);
    store.settle(from, [
      { to: lane, refusal: "This radio's lanes share no clock" },
      { to: { node: "scope", port: "iq" } },
    ]);
    const verdicts = useWiringStore.getState().verdicts;
    const ends = { from, out: iq("iq2", "out"), input: iq("iq", "in") };
    expect(wireRefusal(verdicts, { ...ends, to: lane })).toBe("This radio's lanes share no clock");
    expect(wireRefusal(verdicts, { ...ends, to: { node: "scope", port: "iq" } })).toBeNull();
    expect(wireRefusal(verdicts, { ...ends, to: { node: "rec", port: "iq" } })).toBe(NOT_OFFERED);
  });

  it("drops an answer for a drag that already ended", () => {
    const store = useWiringStore.getState();
    store.start(from);
    store.start({ node: "other", port: "iq" });
    store.settle(from, [{ to: lane }]);
    expect(useWiringStore.getState().verdicts).toBeNull();
    store.clear();
    expect(useWiringStore.getState().from).toBeNull();
  });
});
