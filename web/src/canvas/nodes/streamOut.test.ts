import { describe, expect, it } from "vitest";
import { pictureSize, streamUrl } from "./streamOut";

describe("streamUrl", () => {
  it("opens the served path on this server", () => {
    expect(streamUrl("/api/streams/out", "http://radio.local:8080")).toBe(
      "http://radio.local:8080/api/streams/out",
    );
  });
});

describe("pictureSize", () => {
  it("is empty until a picture is coded", () => {
    expect(pictureSize(null)).toBe("—");
    expect(
      pictureSize({
        node: "out",
        clients: 0,
        pictures: 3,
        width: 720,
        height: 576,
        bytes: 0,
        dropped: 0,
      }),
    ).toBe("720×576");
  });
});
