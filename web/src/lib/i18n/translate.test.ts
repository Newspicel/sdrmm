import { afterEach, describe, expect, it } from "vitest";
import { initPrefs, setLocale } from "../prefs";
import { de } from "./de";
import { en, type MessageKey } from "./en";
import { pickLocale } from "./languages";
import { t, translate } from "./translate";

afterEach(() => {
  localStorage.clear();
});

function placeholders(text: string): string[] {
  return [...text.matchAll(/\{(\w+)\}/g)].map((match) => match[1] ?? "").toSorted();
}

describe("translate", () => {
  it("fills every named value it is given", () => {
    expect(translate(en, "workspace.switchMany", { count: 3 })).toBe("Switch 3 others too?");
    expect(translate(de, "workspace.rename", { name: "Tower" })).toBe("Tower umbenennen");
  });

  it("leaves a placeholder with no value in place", () => {
    expect(translate(en, "workspace.rename", {})).toBe("Rename {name}");
  });

  it("falls back to English for a key the catalog lacks", () => {
    expect(translate({}, "library.settings")).toBe("Settings");
    expect(translate({ "library.settings": "Réglages" }, "library.settings")).toBe("Réglages");
  });
});

describe("t", () => {
  it("speaks the chosen language", () => {
    initPrefs({ lang: "", style: { fontSize: "" } }, ["en"]);
    expect(t("library.settings")).toBe("Settings");
    setLocale("de");
    expect(t("library.settings")).toBe("Einstellungen");
  });
});

describe("German", () => {
  it("has every English key with the same placeholders", () => {
    const keys = Object.keys(en) as MessageKey[];
    expect(Object.keys(de).toSorted()).toEqual(keys.toSorted());
    for (const key of keys) {
      expect(de[key].trim(), key).not.toBe("");
      expect(placeholders(de[key]), key).toEqual(placeholders(en[key]));
    }
  });
});

describe("pickLocale", () => {
  it("matches a browser language on its primary tag", () => {
    expect(pickLocale(["de-CH"])).toBe("de");
    expect(pickLocale(["EN-gb"])).toBe("en");
    expect(pickLocale(["ja", "de"])).toBe("de");
    expect(pickLocale([])).toBe("en");
  });
});
