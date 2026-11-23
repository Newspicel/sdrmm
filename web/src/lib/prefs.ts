import { useSyncExternalStore } from "react";
import { isLocale, type Locale, pickLocale } from "./i18n/languages";
import { isFiniteNumber, readRemembered, rememberedKey, storeRemembered } from "./remembered";

/** Font size in percent of the design size. Kept per browser, never on the server. */
export const TEXT_SIZE = { min: 80, max: 150, step: 5, base: 100 } as const;

export interface Prefs {
  textSize: number;
  locale: Locale;
}

/** The part of the root element the prefs write to. */
export interface PrefsRoot {
  lang: string;
  style: { fontSize: string };
}

const TEXT_SIZE_KEY = rememberedKey("textSize");
const LOCALE_KEY = rememberedKey("locale");
const DEFAULT: Prefs = { textSize: TEXT_SIZE.base, locale: "en" };

const listeners = new Set<() => void>();
let state: Prefs = DEFAULT;
let root: PrefsRoot | null = null;

export function clampTextSize(percent: number): number {
  if (!Number.isFinite(percent)) {
    return TEXT_SIZE.base;
  }
  const stepped = Math.round(percent / TEXT_SIZE.step) * TEXT_SIZE.step;
  return Math.min(TEXT_SIZE.max, Math.max(TEXT_SIZE.min, stepped));
}

export function initPrefs(
  target: PrefsRoot = document.documentElement,
  languages: readonly string[] = navigator.languages,
): void {
  root = target;
  publish({
    textSize: clampTextSize(readRemembered(TEXT_SIZE_KEY, TEXT_SIZE.base, isFiniteNumber)),
    locale: readRemembered(LOCALE_KEY, pickLocale(languages), isLocale),
  });
}

export function setTextSize(percent: number): void {
  const textSize = clampTextSize(percent);
  storeRemembered(TEXT_SIZE_KEY, textSize);
  publish({ ...state, textSize });
}

export function setLocale(locale: Locale): void {
  storeRemembered(LOCALE_KEY, locale);
  publish({ ...state, locale });
}

export function prefs(): Prefs {
  return state;
}

export function usePrefs(): Prefs {
  return useSyncExternalStore(onPrefsChange, prefs, prefs);
}

/** How much larger than its design size the UI draws, for geometry kept in pixels. */
export function uiScale(): number {
  return state.textSize / TEXT_SIZE.base;
}

export function useUiScale(): number {
  return usePrefs().textSize / TEXT_SIZE.base;
}

/// Node faces keep their registry geometry in pixels at the design size. As rem it grows with the
/// font size, so a face keeps its proportions at any size.
export function rem(px: number): string {
  return `${px / 16}rem`;
}

export function onPrefsChange(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function publish(next: Prefs): void {
  state = next;
  if (root !== null) {
    root.style.fontSize = next.textSize === TEXT_SIZE.base ? "" : `${next.textSize}%`;
    root.lang = next.locale;
  }
  for (const listener of listeners) {
    listener();
  }
}
