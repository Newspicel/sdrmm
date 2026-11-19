import {
  type Form,
  fieldShown,
  fieldValue,
  isQuiet,
  type Settings,
  shownNumber,
  shownSpec,
} from "../lib/settingsForm";
import type { SettingField } from "../lib/types";
import { formatHz } from "./format";

export interface SummaryRow {
  key: string;
  label: string;
  value: string;
}

/** What a form is set to, as read-only rows: shown fields that are on, in form order. */
export function settingsSummary(form: Form, settings: Settings): SummaryRow[] {
  return form.flatMap((field) => {
    if (!fieldShown(form, settings, field)) {
      return [];
    }
    const value = fieldValue(field, settings);
    const shown = isQuiet(field, value) ? null : summaryValue(field, value);
    return shown === null ? [] : [{ key: field.key, label: field.label, value: shown }];
  });
}

function summaryValue(field: SettingField, value: unknown): string | null {
  const control = field.control;
  switch (control.type) {
    case "choice":
      return control.options.find((option) => option.value === value)?.label ?? null;
    case "toggle":
      return value === true ? "on" : null;
    case "number":
      return numberText(control, value);
    case "text":
      return typeof value === "string" && value !== "" && control.secret !== true ? value : null;
    default:
      return null;
  }
}

function numberText(spec: Parameters<typeof shownSpec>[0], value: unknown): string | null {
  if (typeof value !== "number") {
    return null;
  }
  if (spec.unit === "Hz") {
    return formatHz(value);
  }
  const unit = shownSpec(spec).unit;
  const shown = shownNumber(spec, value);
  return unit == null || unit === "" ? String(shown) : `${String(shown)} ${unit}`;
}
