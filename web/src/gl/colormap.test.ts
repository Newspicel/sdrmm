import { describe, expect, it } from "vitest";
import {
  COLORMAP_GLSL,
  COLORMAPS,
  DEFAULT_CUSTOM,
  hexToRgb,
  paletteIndex,
  rgbToHex,
  sampleColormap,
  samplePalette,
} from "./colormap";

describe("sampleColormap", () => {
  it("stays inside the unit cube across every ramp", () => {
    for (const map of COLORMAPS) {
      for (let i = 0; i <= 64; i++) {
        for (const channel of sampleColormap(map, i / 64)) {
          expect(channel).toBeGreaterThanOrEqual(0);
          expect(channel).toBeLessThanOrEqual(1);
        }
      }
    }
  });

  it("clamps out-of-range and non-finite inputs to the ramp's ends", () => {
    expect(sampleColormap("gray", -1)).toEqual(sampleColormap("gray", 0));
    expect(sampleColormap("gray", 2)).toEqual(sampleColormap("gray", 1));
    expect(sampleColormap("gray", Number.NaN)).toEqual(sampleColormap("gray", 0));
  });

  it("reads classic's endpoints as its first and last stop", () => {
    expect(sampleColormap("classic", 0)).toEqual([0, 0, 0.12549]);
    expect(sampleColormap("classic", 1)).toEqual([0.2902, 0, 0]);
  });

  it("rises monotonically through gray", () => {
    let previous = -1;
    for (let i = 0; i <= 32; i++) {
      const [value] = sampleColormap("gray", i / 32);
      expect(value).toBeGreaterThan(previous);
      previous = value;
    }
  });

  it("gives every ramp a distinct midpoint", () => {
    const seen = new Set(COLORMAPS.map((map) => sampleColormap(map, 0.5).join()));
    expect(seen.size).toBe(COLORMAPS.length);
  });
});

describe("COLORMAP_GLSL", () => {
  it("branches on each polynomial ramp's index", () => {
    for (const map of ["magma", "inferno", "plasma", "viridis"] as const) {
      expect(COLORMAP_GLSL).toContain(`if (uMap == ${COLORMAPS.indexOf(map)}) { return poly(t,`);
    }
    expect(COLORMAP_GLSL).toContain(
      `if (uMap == ${COLORMAPS.indexOf("gray")}) { return vec3(t); }`,
    );
  });

  it("declares one array entry per classic stop", () => {
    expect(COLORMAP_GLSL).toContain("const vec3 CLASSIC[15] = vec3[15](");
    expect(COLORMAP_GLSL).toContain("vec3(0.00000000, 0.00000000, 0.12549000)");
  });
});

describe("samplePalette", () => {
  it("reads a named ramp like sampleColormap", () => {
    expect(samplePalette("viridis", 0.3)).toEqual(sampleColormap("viridis", 0.3));
  });

  it("blends custom stops linearly", () => {
    const stops = [
      [0, 0, 0],
      [1, 0, 0],
      [1, 1, 1],
    ] as const;
    expect(samplePalette(stops, 0)).toEqual([0, 0, 0]);
    expect(samplePalette(stops, 0.25)).toEqual([0.5, 0, 0]);
    expect(samplePalette(stops, 0.5)).toEqual([1, 0, 0]);
    expect(samplePalette(stops, 2)).toEqual([1, 1, 1]);
  });

  it("gives custom stops the index after every named ramp", () => {
    expect(paletteIndex(DEFAULT_CUSTOM)).toBe(COLORMAPS.length);
    expect(paletteIndex("gray")).toBe(COLORMAPS.indexOf("gray"));
    expect(COLORMAP_GLSL).toContain(`if (uMap == ${COLORMAPS.length}) { return custom(t); }`);
  });
});

describe("hex colours", () => {
  it("round-trips through rgb", () => {
    for (const hex of ["#000000", "#ffffff", "#1e90ff", "#c60000"]) {
      const rgb = hexToRgb(hex);
      expect(rgb).not.toBeNull();
      expect(rgb !== null && rgbToHex(rgb)).toBe(hex);
    }
  });

  it("rejects anything but #rrggbb", () => {
    expect(hexToRgb("#fff")).toBeNull();
    expect(hexToRgb("1e90ff")).toBeNull();
    expect(hexToRgb("#1e90fg")).toBeNull();
  });
});
