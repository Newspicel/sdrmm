import { describe, expect, it } from "vitest";
import {
  aimLabel,
  elevationMax,
  flipped,
  parsedBaud,
  rotctldLink,
  serialLink,
  winegardLink,
  withRange,
} from "./rotator";

describe("aimLabel", () => {
  it("names the bearing and the elevation", () => {
    const aim = { azimuth_deg: 181.4, elevation_deg: 32.6 };
    expect(aimLabel(aim, "azimuth_elevation")).toBe("181° S, 33°");
    expect(aimLabel(aim, "azimuth")).toBe("181° S");
    expect(aimLabel(null, "azimuth_elevation")).toBe("-");
  });

  it("names a bearing past north by its compass point", () => {
    expect(aimLabel({ azimuth_deg: 405, elevation_deg: 0 }, "azimuth")).toBe("405° NE");
  });
});

describe("elevationMax", () => {
  it("reaches over the top only with flip", () => {
    expect(elevationMax("azimuth_elevation", false)).toBe(90);
    expect(elevationMax("azimuth_elevation", true)).toBe(180);
    expect(elevationMax("azimuth", true)).toBe(0);
  });
});

describe("flipped", () => {
  it("stays off for a mount whose elevation cannot pass level", () => {
    const flip = { tolerance_deg: 2, flip: true };
    expect(flipped({ ...flip, link: { type: "gs232", device: "/dev/ttyUSB0", baud: 9_600 } })).toBe(
      true,
    );
    expect(
      flipped({ ...flip, link: { type: "winegard", device: "/dev/ttyUSB0", baud: 57_600 } }),
    ).toBe(false);
    expect(flipped({ tolerance_deg: 2 })).toBe(false);
  });
});

describe("withRange", () => {
  it("pulls saved aims back inside a smaller range", () => {
    const narrowed = withRange(
      {
        tolerance_deg: 2,
        manual: { azimuth_deg: 420, elevation_deg: 10 },
        park: { azimuth_deg: 90, elevation_deg: 0 },
      },
      360,
    );
    expect(narrowed).toEqual({
      azimuth_max_deg: 360,
      manual: { azimuth_deg: 360, elevation_deg: 10 },
      park: { azimuth_deg: 90, elevation_deg: 0 },
    });
  });
});

describe("links", () => {
  it("start from the usual settings", () => {
    expect(serialLink("/dev/ttyUSB0")).toEqual({
      type: "gs232",
      device: "/dev/ttyUSB0",
      baud: 9_600,
    });
    expect(winegardLink("/dev/ttyUSB0")).toEqual({
      type: "winegard",
      device: "/dev/ttyUSB0",
      baud: 57_600,
    });
    expect(rotctldLink()).toEqual({ type: "rotctld", address: "127.0.0.1:4533" });
  });

  it("refuses a baud rate no port runs at", () => {
    expect(parsedBaud("9600")).toBe(9_600);
    expect(parsedBaud("96.5")).toBeNull();
    expect(parsedBaud("12")).toBeNull();
  });
});
