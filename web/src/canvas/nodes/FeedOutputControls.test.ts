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
  it("asks for a wire before anything else", () => {
    expect(feedState(null, false, false)).toBe("Wire events in");
  });

  it("follows the server from closed to listening", () => {
    expect(feedState(null, true, false)).toBe("Server closed");
    expect(feedState(null, true, true)).toBe("Opening server");
    expect(feedState(status, true, true)).toBe("Listening");
  });

  it("leaves a failure to the fault block", () => {
    expect(feedState({ ...status, listening: false, error: "bind" }, true, true)).toBeUndefined();
  });
});
