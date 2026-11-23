import { prefs, usePrefs } from "../prefs";
import { de } from "./de";
import { type Catalog, en, type MessageKey } from "./en";
import type { Locale } from "./languages";

export type { MessageKey } from "./en";

export type Vars = Readonly<Record<string, string | number>>;

export type Translate = (key: MessageKey, vars?: Vars) => string;

const CATALOGS: Record<Locale, Catalog> = { en, de };

/** The text for `key`, English where the catalog lacks it, with each {name} filled from `vars`. */
export function translate(catalog: Partial<Catalog>, key: MessageKey, vars?: Vars): string {
  const text = catalog[key] ?? en[key];
  if (vars === undefined) {
    return text;
  }
  return text.replace(/\{(\w+)\}/g, (whole, name: string) => {
    const value = vars[name];
    return value === undefined ? whole : String(value);
  });
}

/** Outside React; a component uses `useT` so it redraws when the language changes. */
export function t(key: MessageKey, vars?: Vars): string {
  return translate(CATALOGS[prefs().locale], key, vars);
}

export function useT(): Translate {
  const catalog = CATALOGS[usePrefs().locale];
  return (key, vars) => translate(catalog, key, vars);
}
