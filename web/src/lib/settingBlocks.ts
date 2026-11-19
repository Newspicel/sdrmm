import { conditionHolds, type Form, type Settings } from "./settingsForm";
import type { Condition, SettingField } from "./types";

/**
 * Field indexes grouped so each setting sits right under the one it depends on: a field with
 * no `when` on another field of the form starts a block, the fields that need it follow.
 */
export function settingBlocks(form: Form): number[][] {
  const index = new Map(form.map((field, at) => [field.key, at]));
  const blocks = new Map<number, number[]>();
  form.forEach((_, at) => {
    const root = rootOf(form, index, at);
    blocks.set(root, [...(blocks.get(root) ?? []), at]);
  });
  return [...blocks.entries()]
    .toSorted(([left], [right]) => left - right)
    .map(([root, block]) => [root, ...block.filter((at) => at !== root)]);
}

function parentOf(index: ReadonlyMap<string, number>, field: SettingField): number | null {
  const key = field.when?.[0]?.key;
  return key === undefined ? null : (index.get(key) ?? null);
}

function rootOf(form: Form, index: ReadonlyMap<string, number>, at: number): number {
  let current = at;
  const seen = new Set<number>();
  for (;;) {
    const field = form[current];
    const parent = field === undefined ? null : parentOf(index, field);
    if (parent === null || seen.has(parent)) {
      return current;
    }
    seen.add(current);
    current = parent;
  }
}

/** What a greyed field waits for: the setting to change and a short hint naming the value. */
export interface Needs {
  key: string;
  hint: string;
}

export function needsOf(form: Form, settings: Settings, field: SettingField): Needs | null {
  const unmet = (field.when ?? []).find((condition) => !conditionHolds(form, settings, condition));
  if (unmet === undefined) {
    return null;
  }
  const parent = form.find((candidate) => candidate.key === unmet.key);
  const name = parent?.label ?? unmet.key;
  const values = unmet.any_of.map((value) => valueLabel(parent, value));
  return {
    key: unmet.key,
    hint: values.length === 0 ? `Set ${name} first` : `Set ${name} to ${values.join(" or ")}`,
  };
}

function valueLabel(field: SettingField | undefined, value: Condition["any_of"][number]): string {
  const control = field?.control;
  if (control?.type === "choice") {
    const option = control.options.find((candidate) => candidate.value === value);
    if (option !== undefined) {
      return option.label;
    }
  }
  if (typeof value === "boolean") {
    return value ? "Yes" : "No";
  }
  return String(value);
}
