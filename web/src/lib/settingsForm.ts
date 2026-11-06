import { deepEqual } from "./deepEqual";
import type {
  Condition,
  Control,
  LanePick,
  NumberSpec,
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

function isIndex(part: string): boolean {
  return /^\d+$/.test(part);
}

function childAt(value: unknown, part: string): unknown {
  if (Array.isArray(value)) {
    return isIndex(part) ? value[Number(part)] : undefined;
  }
  return isObject(value) ? value[part] : undefined;
}

/**
 * The value at a dotted `key`: `header.length` reads `length` of the `header` object and
 * `stations.1.name` the `name` of the second `stations` entry.
 */
export function valueAt(settings: unknown, key: string): unknown {
  return key.split(".").reduce<unknown>(childAt, settings);
}

function setAt(container: unknown, parts: readonly string[], value: unknown): unknown {
  const [head = "", ...rest] = parts;
  const inner = rest.length === 0 ? value : setAt(childAt(container, head), rest, value);
  if (Array.isArray(container) && isIndex(head)) {
    const copy: unknown[] = [...container];
    copy[Number(head)] = inner;
    return copy;
  }
  return { ...(isObject(container) ? container : {}), [head]: inner };
}

export function withValueAt(settings: Settings, key: string, value: unknown): Settings {
  const [head = "", ...rest] = key.split(".");
  return { ...settings, [head]: rest.length === 0 ? value : setAt(settings[head], rest, value) };
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
  const changed = released(form, pulledDown(form, withValueAt(settings, key, value), key), key);
  return revealed(form, settings, changed);
}

/** Every field held at most the value at `key` comes down to it. */
function pulledDown(form: Form, settings: Settings, key: string): Settings {
  const limit = valueAt(settings, key);
  if (typeof limit !== "number") {
    return settings;
  }
  return form
    .filter((field) => field.at_most === key)
    .reduce<Settings>((next, field) => {
      const held = valueAt(next, field.key);
      return typeof held === "number" && held > limit ? withValueAt(next, field.key, limit) : next;
    }, settings);
}

/** The lane now at `key` leaves every lanes field that may not hold it. */
function released(form: Form, settings: Settings, key: string): Settings {
  const lane = valueAt(settings, key);
  if (typeof lane !== "number") {
    return settings;
  }
  return form.reduce<Settings>((next, field) => {
    const control = field.control;
    if (control.type !== "lanes" || control.except !== key) {
      return next;
    }
    const held = valueAt(next, field.key);
    return isSet(held) ? withValueAt(next, field.key, withoutLane(control.pick, held, lane)) : next;
  }, settings);
}

export function lanesOf(pick: LanePick, value: unknown): number[] {
  switch (pick) {
    case "one":
      return typeof value === "number" ? [value] : [];
    case "list":
      return Array.isArray(value)
        ? value.filter((lane): lane is number => typeof lane === "number")
        : [];
    case "mask":
      return typeof value === "number"
        ? Array.from({ length: 32 }, (_, lane) => lane).filter(
            (lane) => (value & (1 << lane)) !== 0,
          )
        : [];
  }
}

export function lanesValue(pick: LanePick, lanes: readonly number[]): unknown {
  const sorted = [...new Set(lanes)].toSorted((a, b) => a - b);
  switch (pick) {
    case "one":
      return sorted[0] ?? null;
    case "list":
      return sorted;
    case "mask":
      return sorted.reduce((mask, lane) => mask | (1 << lane), 0);
  }
}

function withoutLane(pick: LanePick, value: unknown, lane: number): unknown {
  return pick === "one"
    ? value
    : lanesValue(
        pick,
        lanesOf(pick, value).filter((held) => held !== lane),
      );
}

/** The highest value a field may take now: its spec's, or less under `at_most`. */
export function ceiling(
  field: SettingField,
  settings: Settings,
  max?: number | null,
): number | undefined {
  const limit = field.at_most == null ? undefined : valueAt(settings, field.at_most);
  const spec = max ?? undefined;
  if (typeof limit !== "number") {
    return spec;
  }
  return spec === undefined ? limit : Math.min(spec, limit);
}

/** The fold names of a form, in order of first use. */
export function sections(form: Form): string[] {
  return [...new Set(form.flatMap((field) => (field.section == null ? [] : [field.section])))];
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

/** The text of the running decoder's status readout keyed `key`, if it published one. */
export type OptionStatus = (key: string) => string | undefined;

const NO_STATUS: OptionStatus = () => undefined;

function statusOptions(control: Control): readonly ParamOption[] {
  switch (control.type) {
    case "choice":
      return control.options;
    case "number":
      return control.presets ?? [];
    case "live":
      return statusOptions(control.fallback);
    default:
      return [];
  }
}

/** The status readouts an option of `form` already names, which a face need not repeat. */
export function namedReadouts(form: Form, choices: readonly ParamChoice[]): ReadonlySet<string> {
  return new Set(
    form.flatMap((field) =>
      statusOptions(effectiveControl(field, choices)).flatMap((option) => option.status ?? []),
    ),
  );
}

/** An option's label, followed by the status readout it names while the decoder runs. */
export function optionLabel(option: ParamOption, status: OptionStatus = NO_STATUS): string {
  const text = option.status == null ? undefined : status(option.status);
  return text === undefined || text === "" ? option.label : `${option.label} ${text}`;
}

/** Options keyed by index, with the current value as its own option when none matches. */
export function choiceItems(
  options: readonly ParamOption[],
  value: unknown,
  status: OptionStatus = NO_STATUS,
): ChoiceItems {
  const items = options.map((option, index) => ({
    value: String(index),
    label: optionLabel(option, status),
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
    case "numbers":
      return true;
    case "choice":
    case "live":
    case "lanes":
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

function scaleOf(spec: NumberSpec): number {
  return spec.scale != null && spec.scale > 0 ? spec.scale : 1;
}

function tidy(value: number): number {
  return Number(value.toPrecision(12));
}

function scaled(value: number | null | undefined, by: number): number | null | undefined {
  return value == null ? value : tidy(value / by);
}

/** The number a control shows for a stored `value`: divided by the spec's scale. */
export function shownNumber(spec: NumberSpec, value: unknown): unknown {
  return typeof value === "number" ? scaled(value, scaleOf(spec)) : value;
}

/** The number stored for what a control shows: multiplied back by the spec's scale. */
export function storedNumber(spec: NumberSpec, shown: number | null): number | null {
  return shown === null ? null : tidy(shown * scaleOf(spec));
}

/** `spec` with limits, step and presets in the units the control shows. */
export function shownSpec(spec: NumberSpec): NumberSpec {
  const by = scaleOf(spec);
  if (by === 1) {
    return spec;
  }
  return {
    ...spec,
    min: scaled(spec.min, by),
    max: scaled(spec.max, by),
    step: scaled(spec.step, by),
    presets: spec.presets?.map((preset) => ({ ...preset, value: shownNumber(spec, preset.value) })),
  };
}

export function inRange(value: number, min?: number | null, max?: number | null): boolean {
  return (min == null || value >= min) && (max == null || value <= max);
}
