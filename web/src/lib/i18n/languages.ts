export type Locale = "en" | "de";

/** Each language named in itself, so anyone finds their own. */
export const LANGUAGES: readonly { id: Locale; name: string }[] = [
  { id: "en", name: "English" },
  { id: "de", name: "Deutsch" },
];

export function isLocale(value: unknown): value is Locale {
  return LANGUAGES.some((language) => language.id === value);
}

/** The first browser language we speak, matched on its primary tag, else English. */
export function pickLocale(preferred: readonly string[]): Locale {
  for (const tag of preferred) {
    const primary = tag.split("-")[0]?.toLowerCase();
    if (isLocale(primary)) {
      return primary;
    }
  }
  return "en";
}
