import { deepEqual } from "./deepEqual";
import type {
  Condition,
  Control,
  ParamChoice,
  ParamOption,
  PresetOption,
  SettingField,
} from "./types";

export type Settings = Readonly<Record<string, unknown>>;
export type Form = readonly SettingField[];

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function isSet(value: unknown): boolean {
  return value !== undefined && value !== null;
}

/** The value at a dotted `key` (`header.length` reads `length` of the `header` object). */
export function valueAt(settings: unknown, key: string): unknown {
  return key
    .split(".")
    .reduce<unknown>((value, part) => (isObject(value) ? value[part] : undefined), settings);
}

export function withValueAt(settings: Settings, key: string, value: unknown): Settings {
  const [head = "", ...rest] = key.split(".");
  if (rest.length === 0) {
    return { ...settings, [head]: value };
  }
  const inner = settings[head];
  return { ...settings, [head]: withValueAt(isObject(inner) ? inner : {}, rest.join("."), value) };
}

function fallbackOf(form: Form, key: string): unknown {
  return form.find((field) => field.key === key && isSet(field.fallback))?.fallback;
}

/** What a setting reads as: its own value, else the fallback its field shows while unset. */
export function shownAt(form: Form, settings: Settings, key: string): unknown {
  const value = valueAt(settings, key);
  return isSet(value) ? value : (fallbackOf(form, key) ?? null);
}

export function conditionHolds(form: Form, settings: Settings, condition: Condition): boolean {
  if (condition.any_of.length === 0) {
    return isSet(valueAt(settings, condition.key));
  }
  const value = shownAt(form, settings, condition.key);
  return condition.any_of.some((candidate) => deepEqual(candidate, value));
}

export function fieldShown(form: Form, settings: Settings, field: SettingField): boolean {
  return (field.when ?? []).every((condition) => conditionHolds(form, settings, condition));
}

export function shownFields(form: Form, settings: Settings): SettingField[] {
  return form.filter((field) => fieldShown(form, settings, field));
}

export function fieldValue(field: SettingField, settings: Settings): unknown {
  const value = valueAt(settings, field.key);
  return isSet(value) ? value : (field.fallback ?? null);
}

export function isQuiet(field: SettingField, value: unknown): boolean {
  const shown = value === null && field.control.type === "list" ? [] : value;
  return (field.quiet ?? []).some((quiet) => deepEqual(quiet, shown));
}

/**
 * `settings` with `key` set to `value`, plus the fallback of every unset field the change
 * reveals, so a mode switched on arrives with its own setting.
 */
export function committed(form: Form, settings: Settings, key: string, value: unknown): Settings {
  return revealed(form, settings, withValueAt(settings, key, value));
}

function revealed(form: Form, before: Settings, after: Settings): Settings {
  let next = after;
  for (const field of form) {
    if (!isSet(field.fallback) || isSet(valueAt(next, field.key))) {
      continue;
    }
    if (!fieldShown(form, before, field) && fieldShown(form, next, field)) {
      next = withValueAt(next, field.key, field.fallback);
    }
  }
  return next;
}

/** The control a field renders now: a live field's published options, else its stand-in. */
export function effectiveControl(field: SettingField, choices: readonly ParamChoice[]): Control {
  if (field.control.type !== "live") {
    return field.control;
  }
  const options = choices.find((choice) => choice.name === field.key)?.options ?? [];
  return options.length > 0 ? { type: "choice", options } : field.control.fallback;
}

export const CURRENT = "current";

export interface ChoiceItems {
  options: { value: string; label: string; title?: string }[];
  chosen: string;
}

export function valueLabel(value: unknown): string {
  if (value === null || value === undefined) {
    return "none";
  }
  return typeof value === "string" ? value : JSON.stringify(value);
}

/** Options keyed by index, with the current value as its own option when none matches. */
export function choiceItems(options: readonly ParamOption[], value: unknown): ChoiceItems {
  const items = options.map((option, index) => ({
    value: String(index),
    label: option.label,
    ...(option.title == null ? {} : { title: option.title }),
  }));
  const index = options.findIndex((option) => deepEqual(option.value, value));
  if (index !== -1) {
    return { options: items, chosen: String(index) };
  }
  return {
    options: [...items, { value: CURRENT, label: valueLabel(value) }],
    chosen: CURRENT,
  };
}

export function chosenValue(
  options: readonly ParamOption[],
  key: string,
  current: unknown,
): unknown {
  return key === CURRENT ? current : (options[Number(key)]?.value ?? current);
}

export function presetIndex(
  form: Form,
  settings: Settings,
  options: readonly PresetOption[],
): number {
  return options.findIndex((option) =>
    Object.entries(option.values).every(([key, value]) =>
      deepEqual(shownAt(form, settings, key), value),
    ),
  );
}

export function withPreset(form: Form, settings: Settings, option: PresetOption): Settings {
  const after = Object.entries(option.values).reduce<Settings>(
    (next, [key, value]) => withValueAt(next, key, value),
    settings,
  );
  return revealed(form, settings, after);
}

function filled(field: SettingField, value: unknown): boolean {
  switch (field.control.type) {
    case "text":
      return field.control.optional === true || (typeof value === "string" && value !== "");
    case "number":
      return isSet(field.control.optional) || isSet(value);
    case "toggle":
    case "list":
    case "preset":
      return true;
    case "choice":
    case "live":
      return isSet(value);
  }
}

/** A list entry is complete once every visible field that is not optional holds a value. */
export function entryComplete(item: Form, entry: Settings): boolean {
  return shownFields(item, entry).every((field) => filled(field, valueAt(entry, field.key)));
}

/** A fresh list entry: the fallback of each field it shows. */
export function blankEntry(item: Form): Settings {
  return item.reduce<Settings>(
    (entry, field) =>
      isSet(field.fallback) && fieldShown(item, entry, field)
        ? withValueAt(entry, field.key, field.fallback)
        : entry,
    {},
  );
}

export function listEntries(value: unknown): Settings[] {
  return Array.isArray(value) ? value.filter(isObject) : [];
}

const SCALE: Readonly<Record<string, number>> = { "": 1, k: 1e3, m: 1e6, g: 1e9 };

/** Hertz from `12500`, `2.4 kHz` or `145.5M`; a bare number is hertz. */
export function parseHz(text: string): number | null {
  const match = /^\s*(-?[0-9]*[.,]?[0-9]+)\s*([kmg]?)(?:hz)?\s*$/i.exec(text);
  if (match === null) {
    return null;
  }
  const value = Number(match[1]?.replace(",", ".")) * (SCALE[match[2]?.toLowerCase() ?? ""] ?? 1);
  return Number.isFinite(value) ? value : null;
}

export function inRange(value: number, min?: number | null, max?: number | null): boolean {
  return (min == null || value >= min) && (max == null || value <= max);
}
