import { afterEach, describe, expect, it } from "vitest";
import { useBroadcastStore } from "./broadcast";
import type { BroadcastStatus, ServerEvent } from "./types";

function update(node: string, status: BroadcastStatus): ServerEvent {
  return { type: "ChannelBroadcast", data: { node, status } };
}

const SEARCHING: BroadcastStatus = {
  system: "dvb_t",
  locked: false,
  snr_db: 0,
  frequency_error_hz: 0,
};

afterEach(() => useBroadcastStore.getState().reset());

describe("useBroadcastStore", () => {
  it("keeps only the latest status per channel node", () => {
    const { observe } = useBroadcastStore.getState();
    observe(update("dvbt", SEARCHING));
    observe(update("dvbt", { ...SEARCHING, locked: true, snr_db: 21 }));
    observe(update("dab", SEARCHING));
    const byNode = useBroadcastStore.getState().byNode;
    expect(Object.keys(byNode)).toHaveLength(2);
    expect(byNode.dvbt?.locked).toBe(true);
  });

  it("ignores other server events", () => {
    useBroadcastStore.getState().observe({ type: "DecodedLost", data: { count: 1 } });
    expect(useBroadcastStore.getState().byNode).toEqual({});
  });
});
