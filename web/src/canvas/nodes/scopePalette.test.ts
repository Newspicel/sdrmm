import { afterEach, describe, expect, it } from "vitest";
import { DEFAULT_COLORMAP, DEFAULT_CUSTOM } from "../../gl/colormap";
import {
  CUSTOM,
  paletteOf,
  parseCustom,
  readChoice,
  readCustom,
  readPalette,
  storeChoice,
  storeCustom,
} from "./scopePalette";

const RED_GREEN_BLUE = [
  [1, 0, 0],
  [0, 1, 0],
  [0, 0, 1],
] as const;

afterEach(() => localStorage.clear());

describe("scope palette", () => {
  it("falls back to defaults when nothing is stored", () => {
    expect(readChoice()).toBe(DEFAULT_COLORMAP);
    expect(readCustom()).toBe(DEFAULT_CUSTOM);
  });

  it("round-trips the choice and custom stops", () => {
    storeChoice(CUSTOM);
    storeCustom(RED_GREEN_BLUE);
    expect(readChoice()).toBe(CUSTOM);
    expect(readCustom()).toEqual(RED_GREEN_BLUE);
    expect(readPalette()).toEqual(RED_GREEN_BLUE);
  });

  it("uses the named ramp unless custom is chosen", () => {
    expect(paletteOf("magma", RED_GREEN_BLUE)).toBe("magma");
    expect(paletteOf(CUSTOM, RED_GREEN_BLUE)).toBe(RED_GREEN_BLUE);
  });

  it("rejects malformed stored stops", () => {
    expect(parseCustom(null)).toBeNull();
    expect(parseCustom("not json")).toBeNull();
    expect(parseCustom('["#000000","#ffffff"]')).toBeNull();
    expect(parseCustom('["#000000","#ffffff",3]')).toBeNull();
    expect(parseCustom('["#000000","#ffffff","#zzzzzz"]')).toBeNull();
  });
});
