import { describe, expect, it } from "vitest";
import type { FeedStatus } from "../../lib/types";
import { feedState } from "./FeedOutputControls";

const status: FeedStatus = {
  node: "feed",
  format: "probe",
  address: "127.0.0.1:30999",
  listening: true,
  clients: 2,
  frames: 10,
};

describe("feedState", () => {
  it("says nothing while unwired", () => {
    expect(feedState(null, false, false)).toBeUndefined();
  });

  it("follows the server from closed to listening", () => {
    expect(feedState(null, true, false)).toBe("Server closed");
    expect(feedState(null, true, true)).toBe("Opening server");
    expect(feedState(status, true, true)).toBe("Listening");
  });

  it("shows what the server waits for as a hint", () => {
    const waiting = "No decoder offers the probe feed";
    expect(feedState({ ...status, listening: false, waiting }, true, true)).toBe(waiting);
  });

  it("leaves a failure to the fault block", () => {
    expect(feedState({ ...status, listening: false, error: "bind" }, true, true)).toBeUndefined();
  });
});
