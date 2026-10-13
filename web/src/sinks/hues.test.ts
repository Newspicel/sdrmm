import { describe, expect, it } from "vitest";
import { CATALOG } from "../test/catalog";
import { HUE_COLOR, HUE_TEXT, portHue } from "./hues";

describe("portHue", () => {
  it("colours every port type the server serves", () => {
    for (const info of CATALOG.port_types) {
      expect(HUE_TEXT[portHue(CATALOG, info.port_type)]).toBeTruthy();
      expect(HUE_COLOR[portHue(CATALOG, info.port_type)]).toMatch(/^var\(--color-/);
    }
  });

  it("covers baseband and position", () => {
    expect(portHue(CATALOG, "baseband")).toBe("cyan");
    expect(portHue(CATALOG, "position")).toBe("accent");
  });

  it("falls back to muted for a type the catalog lacks", () => {
    expect(portHue({ nodes: [], port_types: [], event_kinds: [] }, "iq")).toBe("muted");
  });
});
