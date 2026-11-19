import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { domeRadius, passPath, SkyDome } from "./SkyDome";

describe("SkyDome", () => {
  it("puts the horizon on the rim and the zenith in the middle", () => {
    expect(domeRadius(0)).toBe(40);
    expect(domeRadius(90)).toBe(0);
    expect(domeRadius(-10)).toBe(40);
  });

  it("draws only the part of a pass above the horizon", () => {
    const path = passPath([
      { at: 0, azimuth_deg: 0, elevation_deg: -5 },
      { at: 1, azimuth_deg: 90, elevation_deg: 0 },
      { at: 2, azimuth_deg: 180, elevation_deg: 90 },
    ]);
    expect(path.startsWith("M90.00 50.00")).toBe(true);
    expect(path).toContain("L50.00 50.00");
  });

  it("keeps its shape with nothing to show", () => {
    const html = renderToStaticMarkup(<SkyDome look={null} track={[]} visible={false} />);
    expect(html.match(/<circle/g)).toHaveLength(3);
    expect(html).not.toContain("Where the satellite is now");
  });
});

describe("SkyDome quiet", () => {
  it("keeps only the horizon so a hint can sit in the middle", () => {
    const html = renderToStaticMarkup(<SkyDome look={null} track={[]} visible={false} quiet />);
    expect(html.match(/<circle/g)).toHaveLength(1);
  });
});
