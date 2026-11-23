import { afterEach, describe, expect, it, vi } from "vitest";
import {
  clampTextSize,
  initPrefs,
  onPrefsChange,
  type PrefsRoot,
  prefs,
  rem,
  setLocale,
  setTextSize,
  TEXT_SIZE,
  uiScale,
} from "./prefs";

function root(): PrefsRoot {
  return { lang: "", style: { fontSize: "" } };
}

afterEach(() => {
  localStorage.clear();
});

describe("clampTextSize", () => {
  it("keeps the size on a step inside the range", () => {
    expect(clampTextSize(100)).toBe(100);
    expect(clampTextSize(112)).toBe(110);
    expect(clampTextSize(113)).toBe(115);
    expect(clampTextSize(10)).toBe(TEXT_SIZE.min);
    expect(clampTextSize(900)).toBe(TEXT_SIZE.max);
    expect(clampTextSize(Number.NaN)).toBe(TEXT_SIZE.base);
  });
});

describe("initPrefs", () => {
  it("restores what this browser kept and writes it to the root", () => {
    localStorage.setItem("sdrmm.textSize", "125");
    localStorage.setItem("sdrmm.locale", '"de"');
    const target = root();
    initPrefs(target, ["en-US"]);
    expect(prefs()).toEqual({ textSize: 125, locale: "de" });
    expect(target.style.fontSize).toBe("125%");
    expect(target.lang).toBe("de");
    expect(uiScale()).toBe(1.25);
  });

  it("takes the browser language when nothing was kept", () => {
    initPrefs(root(), ["de-AT", "en"]);
    expect(prefs().locale).toBe("de");
    initPrefs(root(), ["fr-FR", "es"]);
    expect(prefs().locale).toBe("en");
  });

  it("falls back on stale or broken values", () => {
    localStorage.setItem("sdrmm.textSize", '"huge"');
    localStorage.setItem("sdrmm.locale", '"fr"');
    const target = root();
    initPrefs(target, ["en"]);
    expect(prefs()).toEqual({ textSize: TEXT_SIZE.base, locale: "en" });
    expect(target.style.fontSize).toBe("");

    localStorage.setItem("sdrmm.textSize", "400");
    localStorage.setItem("sdrmm.locale", "{nope");
    initPrefs(target, ["de"]);
    expect(prefs()).toEqual({ textSize: TEXT_SIZE.max, locale: "de" });
  });
});

describe("setters", () => {
  it("keep the choice, write the root and tell listeners", () => {
    const target = root();
    initPrefs(target, ["en"]);
    const heard = vi.fn();
    const stop = onPrefsChange(heard);

    setTextSize(93);
    expect(localStorage.getItem("sdrmm.textSize")).toBe("95");
    expect(target.style.fontSize).toBe("95%");

    setLocale("de");
    expect(localStorage.getItem("sdrmm.locale")).toBe('"de"');
    expect(target.lang).toBe("de");

    setTextSize(TEXT_SIZE.base);
    expect(target.style.fontSize).toBe("");
    expect(heard).toHaveBeenCalledTimes(3);
    stop();
  });
});

describe("rem", () => {
  it("turns design pixels into rem", () => {
    expect(rem(280)).toBe("17.5rem");
  });
});
