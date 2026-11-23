import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it } from "vitest";
import { initPrefs, setLocale } from "../lib/prefs";
import { PrefsPanel } from "./PrefsPanel";

afterEach(() => {
  localStorage.clear();
});

describe("PrefsPanel", () => {
  it("shows theme, font size and language in the chosen language", () => {
    initPrefs({ lang: "", style: { fontSize: "" } }, ["en"]);
    const english = renderToStaticMarkup(<PrefsPanel />);
    for (const text of ["Theme", "Font size", "Language", "100 %", "English"]) {
      expect(english).toContain(text);
    }
    setLocale("de");
    const german = renderToStaticMarkup(<PrefsPanel />);
    for (const text of ["Design", "Schriftgröße", "Sprache", "Deutsch"]) {
      expect(german).toContain(text);
    }
  });
});
