import { useEffect } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  isBoolean,
  isIndex,
  listOf,
  oneOf,
  orNull,
  parseRemembered,
  readRemembered,
  rememberedKey,
  storeRemembered,
  useRemembered,
  within,
} from "./remembered";

const IS_SIDE = oneOf(["left", "right"] as const);

afterEach(() => {
  vi.unstubAllGlobals();
  localStorage.clear();
});

describe("rememberedKey", () => {
  it("scopes by workspace and node", () => {
    expect(rememberedKey("view")).toBe("sdrmm.view");
    expect(rememberedKey("scopeView", 3, "scope1")).toBe("sdrmm.scopeView:3:scope1");
  });
});

describe("parseRemembered", () => {
  it("falls back on missing, broken or stale values", () => {
    expect(parseRemembered(null, "left", IS_SIDE)).toBe("left");
    expect(parseRemembered("{nope", "left", IS_SIDE)).toBe("left");
    expect(parseRemembered('"up"', "left", IS_SIDE)).toBe("left");
    expect(parseRemembered('"right"', "left", IS_SIDE)).toBe("right");
  });
});

describe("validators", () => {
  it("accept only what fits", () => {
    expect(within(0, 1)(0.5)).toBe(true);
    expect(within(0, 1)(2)).toBe(false);
    expect(within(0, 1)(Number.NaN)).toBe(false);
    expect(isIndex(2)).toBe(true);
    expect(isIndex(1.5)).toBe(false);
    expect(isIndex(-1)).toBe(false);
    expect(isBoolean("true")).toBe(false);
    expect(listOf(IS_SIDE)(["left", "right"])).toBe(true);
    expect(listOf(IS_SIDE)(["left", 1])).toBe(false);
    expect(orNull(IS_SIDE)(null)).toBe(true);
  });
});

describe("storeRemembered", () => {
  it("round-trips through localStorage", () => {
    const key = rememberedKey("side");
    storeRemembered(key, "right");
    expect(localStorage.getItem(key)).toBe('"right"');
    expect(readRemembered(key, "left", IS_SIDE)).toBe("right");
  });

  it("holds the choice for the page when storage refuses", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => {
        throw new Error("full");
      },
    });
    const key = rememberedKey("refused");
    storeRemembered(key, "right");
    expect(readRemembered(key, "left", IS_SIDE)).toBe("right");
  });
});

function Side({ name, pick }: { name: string; pick?: "left" | "right" }) {
  const [side, setSide] = useRemembered(rememberedKey(name), "left", IS_SIDE);
  useEffect(() => {
    if (pick !== undefined) {
      setSide(pick);
    }
  });
  return side;
}

describe("useRemembered", () => {
  it("starts from the stored choice", () => {
    expect(renderToStaticMarkup(<Side name="fresh" />)).toBe("left");
    storeRemembered(rememberedKey("held"), "right");
    expect(renderToStaticMarkup(<Side name="held" />)).toBe("right");
    localStorage.setItem(rememberedKey("stale"), '"up"');
    expect(renderToStaticMarkup(<Side name="stale" />)).toBe("left");
  });
});
