import { afterEach, describe, expect, it, vi } from "vitest";
import { clientEvents, resetEvents } from "../diagnostics";
import {
  blankStyle,
  chooseBasemap,
  DEFAULT_BASEMAP,
  fetchStyle,
  groundOf,
  isTileTemplate,
  loadBasemap,
  presetUrl,
  sameBasemap,
  withCustom,
} from "./basemap";

const BACKGROUND = "#101113";

describe("chooseBasemap", () => {
  it("takes the online style whenever one came back", () => {
    const online = blankStyle("#000");
    const chosen = chooseBasemap(online, BACKGROUND);
    expect(chosen.kind).toBe("online");
    expect(chosen.style).toBe(online);
  });

  it("says plainly when there is nothing to draw", () => {
    const chosen = chooseBasemap(null, BACKGROUND);
    expect(chosen.kind).toBe("blank");
    expect(chosen.style.layers).toHaveLength(1);
  });
});

describe("fetchStyle", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    resetEvents();
  });

  it("records why the online style is missing", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("", { status: 503 })));
    expect(await fetchStyle(presetUrl("liberty"))).toBeNull();
    expect(clientEvents()).toContainEqual(
      expect.objectContaining({ level: "warn", source: "map", message: "basemap style: HTTP 503" }),
    );
  });
});

describe("loadBasemap", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    resetEvents();
  });

  it("fetches the chosen preset", async () => {
    const fetch = vi.fn().mockResolvedValue(Response.json(blankStyle("#123")));
    vi.stubGlobal("fetch", fetch);
    const chosen = await loadBasemap({ preset: "dark", custom: "" }, BACKGROUND);
    expect(chosen.kind).toBe("online");
    expect(fetch).toHaveBeenCalledWith(presetUrl("dark"), expect.anything());
  });

  it("builds a raster style from an XYZ template without fetching", async () => {
    const fetch = vi.fn();
    vi.stubGlobal("fetch", fetch);
    const template = "https://tiles.example/{z}/{x}/{y}.jpg?key=abc";
    const chosen = await loadBasemap({ preset: "custom", custom: template }, BACKGROUND);
    expect(chosen.kind).toBe("online");
    expect(chosen.style.sources.custom).toMatchObject({ type: "raster", tiles: [template] });
    expect(chosen.style.glyphs).toBeDefined();
    expect(fetch).not.toHaveBeenCalled();
  });

  it("fetches a custom style URL", async () => {
    const fetch = vi.fn().mockResolvedValue(Response.json(blankStyle("#123")));
    vi.stubGlobal("fetch", fetch);
    const url = "https://maps.example/style.json?key=abc";
    await loadBasemap({ preset: "custom", custom: url }, BACKGROUND);
    expect(fetch).toHaveBeenCalledWith(url, expect.anything());
  });

  it("falls back to blank for a URL that is not http(s)", async () => {
    const chosen = await loadBasemap({ preset: "custom", custom: "file:///x" }, BACKGROUND);
    expect(chosen.kind).toBe("blank");
    expect(clientEvents()).toContainEqual(expect.objectContaining({ source: "map" }));
  });
});

describe("groundOf", () => {
  it("is the land of the preset that loaded, else the page behind a blank map", () => {
    expect(groundOf({ preset: "dark", custom: "" }, "online", BACKGROUND)).toBe("#0c0c0c");
    expect(groundOf(DEFAULT_BASEMAP, "online", BACKGROUND)).toBe("#f8f4f0");
    expect(groundOf({ preset: "custom", custom: "x" }, "online", BACKGROUND)).toBe("#f8f4f0");
    expect(groundOf(DEFAULT_BASEMAP, "blank", BACKGROUND)).toBe(BACKGROUND);
  });
});

describe("basemap choice", () => {
  it("tells tile templates from style URLs", () => {
    expect(isTileTemplate("https://a/{z}/{x}/{y}.png")).toBe(true);
    expect(isTileTemplate("https://a/q/{quadkey}.png")).toBe(true);
    expect(isTileTemplate("https://a/style.json")).toBe(false);
  });

  it("ignores the stored URL unless custom is picked", () => {
    expect(sameBasemap({ preset: "dark", custom: "a" }, { preset: "dark", custom: "b" })).toBe(
      true,
    );
    expect(sameBasemap({ preset: "custom", custom: "a" }, { preset: "custom", custom: "b" })).toBe(
      false,
    );
  });

  it("picks custom when a URL is entered and leaves it when cleared", () => {
    expect(withCustom({ preset: "dark", custom: "" }, "https://x")).toEqual({
      preset: "custom",
      custom: "https://x",
    });
    expect(withCustom({ preset: "custom", custom: "https://x" }, "")).toEqual(DEFAULT_BASEMAP);
    expect(withCustom({ preset: "dark", custom: "https://x" }, "")).toEqual({
      preset: "dark",
      custom: "",
    });
  });
});
