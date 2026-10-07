import { afterEach, describe, expect, it } from "vitest";
import { useBroadcastStore } from "./broadcast";
import type { BroadcastStatus, ServerEvent } from "./types";

function update(channel: number, status: BroadcastStatus): ServerEvent {
  return { type: "BroadcastUpdate", data: { device_set: 1, channel, status } };
}

const SEARCHING: BroadcastStatus = {
  system: "dvb_t",
  locked: false,
  snr_db: 0,
  frequency_error_hz: 0,
};

afterEach(() => useBroadcastStore.getState().reset());

describe("useBroadcastStore", () => {
  it("keeps only the latest status per channel", () => {
    const { observe } = useBroadcastStore.getState();
    observe(update(2, SEARCHING));
    observe(update(2, { ...SEARCHING, locked: true, snr_db: 21 }));
    observe(update(3, SEARCHING));
    const byChannel = useBroadcastStore.getState().byChannel;
    expect(Object.keys(byChannel)).toHaveLength(2);
    expect(byChannel["1:2"]?.status.locked).toBe(true);
  });

  it("ignores other server events", () => {
    useBroadcastStore.getState().observe({ type: "DecodedLost", data: { count: 1 } });
    expect(useBroadcastStore.getState().byChannel).toEqual({});
  });
});
