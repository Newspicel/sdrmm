import { afterEach, describe, expect, it } from "vitest";
import { statusChoices, useChannelStatusStore } from "./channelStatus";
import type { ChannelStatus, ServerEvent } from "./types";

function update(node: string, status: ChannelStatus): ServerEvent {
  return { type: "ChannelStatus", data: { node, status } };
}

const SEARCHING: ChannelStatus = {
  readouts: [{ key: "lock", label: "Lock", value: { type: "flag", value: false } }],
};

afterEach(() => useChannelStatusStore.getState().reset());

describe("useChannelStatusStore", () => {
  it("keeps only the latest status per channel node", () => {
    const { observe } = useChannelStatusStore.getState();
    observe(update("a", SEARCHING));
    observe(
      update("a", {
        readouts: [{ key: "lock", label: "Lock", value: { type: "flag", value: true } }],
      }),
    );
    observe(update("b", SEARCHING));
    const byNode = useChannelStatusStore.getState().byNode;
    expect(Object.keys(byNode)).toHaveLength(2);
    expect(byNode.a?.readouts?.[0]?.value).toEqual({ type: "flag", value: true });
  });

  it("ignores other server events", () => {
    useChannelStatusStore.getState().observe({ type: "DecodedLost", data: { count: 1 } });
    expect(useChannelStatusStore.getState().byNode).toEqual({});
  });

  it("offers the choices a status publishes, none without one", () => {
    const choices = [{ name: "service", options: [{ label: "One", value: 1 }] }];
    expect(statusChoices({ choices })).toBe(choices);
    expect(statusChoices(undefined)).toEqual([]);
  });
});
