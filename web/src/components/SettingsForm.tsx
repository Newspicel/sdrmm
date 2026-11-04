import {
  ceiling,
  choiceItems,
  chosenValue,
  committed,
  effectiveControl,
  type Form,
  fieldShown,
  fieldValue,
  isQuiet,
  listEntries,
  type OptionStatus,
  optionLabel,
  presetIndex,
  type Settings,
  shownNumber,
  shownSpec,
  storedNumber,
  valueAt,
  withPreset,
} from "../lib/settingsForm";
import type {
  Field,
  NumberSpec,
  ParamChoice,
  ParamOption,
  PresetOption,
  SettingField,
} from "../lib/types";
import { formatValue } from "../sinks/values";
import type { Options } from "./controls";
import {
  ChipField,
  ChoiceChip,
  NumberChip,
  OptionalNumberChip,
  SettingChip,
  ToggleChip,
} from "./face/Chips";
import { TextChip } from "./face/TextChip";
import { HzChip } from "./HzField";
import { LanesChip, NumbersChip } from "./LaneChips";
import { ListChip } from "./ListChip";
import { NumberField } from "./NumberField";
import { rowKeys } from "./rowKeys";
import { Segmented } from "./Segmented";

const NO_READOUTS: readonly Field[] = [];

function readoutStatus(readouts: readonly Field[]): OptionStatus {
  return (key) => {
    const readout = readouts.find((field) => field.key === key);
    return readout === undefined ? undefined : formatValue(readout.value);
  };
}

/** A number spec as its control shows it: scaled, presets labelled with their status. */
function displaySpec(spec: NumberSpec, status: OptionStatus): NumberSpec {
  const shown = shownSpec(spec);
  return {
    ...shown,
    presets: shown.presets?.map((preset) => ({ ...preset, label: optionLabel(preset, status) })),
  };
}

/**
 * A decoder's settings as chips, drawn from the form its descriptor publishes. `readouts`
 * are the running decoder's status, which options name to show what `Auto` found. With
 * `section`, only that fold's fields; `null` picks the fields outside every fold.
 */
export function SettingsForm({
  form,
  settings,
  choices,
  readouts = NO_READOUTS,
  section,
  lanes = 0,
  onSettings,
}: {
  form: Form;
  settings: Settings;
  choices: readonly ParamChoice[];
  readouts?: readonly Field[];
  section?: string | null;
  lanes?: number;
  onSettings: (settings: Settings) => void;
}) {
  const keys = rowKeys(form);
  const status = readoutStatus(readouts);
  return form.map((field, index) =>
    inSection(field, section) && fieldShown(form, settings, field) ? (
      <FieldChip
        key={keys[index]}
        form={form}
        field={field}
        settings={settings}
        choices={choices}
        status={status}
        lanes={lanes}
        onSettings={onSettings}
      />
    ) : null,
  );
}

function inSection(field: SettingField, section: string | null | undefined): boolean {
  return section === undefined || (field.section ?? null) === section;
}

interface FieldProps {
  form: Form;
  field: SettingField;
  settings: Settings;
  choices: readonly ParamChoice[];
  status: OptionStatus;
  lanes: number;
  onSettings: (settings: Settings) => void;
}

function belowCeiling(options: readonly ParamOption[], top: number | undefined): ParamOption[] {
  return options.filter(
    (option) => top === undefined || typeof option.value !== "number" || option.value <= top,
  );
}

function FieldChip({ form, field, settings, choices, status, lanes, onSettings }: FieldProps) {
  const control = effectiveControl(field, choices);
  const value = fieldValue(field, settings);
  const shared = {
    label: field.label,
    title: field.title ?? field.label,
    quiet: isQuiet(field, value),
  };
  const commit = (next: unknown): void => onSettings(committed(form, settings, field.key, next));
  switch (control.type) {
    case "choice":
      return (
        <OptionChip
          {...shared}
          options={belowCeiling(control.options, ceiling(field, settings))}
          value={value}
          status={status}
          onCommit={commit}
        />
      );
    case "number":
      return (
        <SpecChip
          {...shared}
          spec={displaySpec(
            { ...control, max: ceiling(field, settings, control.max) ?? null },
            status,
          )}
          value={shownNumber(control, value)}
          onCommit={(shown) => commit(storedNumber(control, shown))}
        />
      );
    case "lanes": {
      const taken = control.except == null ? null : valueAt(settings, control.except);
      return (
        <LanesChip
          {...shared}
          pick={control.pick}
          value={value}
          lanes={lanes}
          except={typeof taken === "number" ? taken : null}
          empty={control.empty ?? undefined}
          onCommit={commit}
        />
      );
    }
    case "numbers":
      return (
        <NumbersChip
          {...shared}
          spec={control.spec}
          maxLen={control.max_len}
          value={value}
          onCommit={commit}
        />
      );
    case "toggle":
      return (
        <ToggleChip
          label={shared.label}
          title={shared.title}
          on={value === true}
          onChange={commit}
        />
      );
    case "text":
      return (
        <TextChip
          {...shared}
          value={typeof value === "string" ? value : ""}
          placeholder={control.placeholder ?? undefined}
          secret={control.secret === true}
          maxLength={control.max_len ?? undefined}
          onCommit={(text) => commit(text === "" && control.optional === true ? null : text)}
        />
      );
    case "preset":
      return (
        <PresetsChip
          {...shared}
          options={control.options}
          chosen={presetIndex(form, settings, control.options)}
          onPick={(option) => onSettings(withPreset(form, settings, option))}
        />
      );
    case "list":
      return (
        <ListChip
          {...shared}
          item={control.item}
          entries={listEntries(value)}
          empty={control.empty}
          onChange={commit}
        />
      );
    case "live":
      return null;
  }
}

interface ChipProps {
  label: string;
  title: string;
  quiet: boolean;
}

function OptionChip({
  options,
  value,
  status,
  onCommit,
  ...chip
}: ChipProps & {
  options: readonly ParamOption[];
  value: unknown;
  status: OptionStatus;
  onCommit: (value: unknown) => void;
}) {
  const items = choiceItems(options, value, status);
  return (
    <ChoiceChip
      {...chip}
      value={items.chosen}
      options={items.options}
      onChange={(key) => onCommit(chosenValue(options, key, value))}
    />
  );
}

const CUSTOM = -1;

function PresetsChip({
  options,
  chosen,
  onPick,
  ...chip
}: ChipProps & {
  options: readonly PresetOption[];
  chosen: number;
  onPick: (option: PresetOption) => void;
}) {
  const items: Options<number> = [
    ...options.map((option, index) => ({
      value: index,
      label: option.label,
      ...(option.title == null ? {} : { title: option.title }),
    })),
    { value: CUSTOM, label: "Custom", disabled: true },
  ];
  return (
    <ChoiceChip
      {...chip}
      value={chosen}
      options={items}
      onChange={(index) => {
        const option = options[index];
        if (option !== undefined) {
          onPick(option);
        }
      }}
    />
  );
}

function SpecChip({
  spec,
  value,
  onCommit,
  ...chip
}: ChipProps & {
  spec: NumberSpec;
  value: unknown;
  onCommit: (value: number | null) => void;
}) {
  const number = typeof value === "number" ? value : null;
  const limits = { min: spec.min ?? undefined, max: spec.max ?? undefined };
  const optional = spec.optional ?? undefined;
  if (spec.unit === "Hz") {
    return (
      <HzChip
        {...chip}
        {...limits}
        value={number}
        placeholder={optional}
        optional={optional !== undefined}
        onCommit={onCommit}
      />
    );
  }
  const unit = spec.unit ?? undefined;
  const step = spec.step ?? undefined;
  if (optional !== undefined || number === null) {
    return (
      <OptionalNumberChip
        {...chip}
        {...limits}
        value={number}
        placeholder={optional ?? "none"}
        unit={unit}
        step={step}
        onCommit={onCommit}
      />
    );
  }
  const presets = (spec.presets ?? []).filter(
    (preset): preset is ParamOption & { value: number } => typeof preset.value === "number",
  );
  if (presets.length > 0) {
    return (
      <PresetNumberChip
        {...chip}
        {...limits}
        value={number}
        unit={unit}
        step={step}
        presets={presets}
        onCommit={onCommit}
      />
    );
  }
  return (
    <NumberChip {...chip} {...limits} value={number} unit={unit} step={step} onCommit={onCommit} />
  );
}

function PresetNumberChip({
  label,
  title,
  quiet,
  value,
  presets,
  unit,
  min,
  max,
  step,
  onCommit,
}: ChipProps & {
  value: number;
  presets: readonly (ParamOption & { value: number })[];
  unit?: string;
  min?: number;
  max?: number;
  step?: number;
  onCommit: (value: number) => void;
}) {
  const options: Options<number> = presets.map((preset) => ({
    value: preset.value,
    label: preset.label,
  }));
  return (
    <SettingChip label={label} value={String(value)} unit={unit} title={title} quiet={quiet}>
      {() => (
        <ChipField label={title}>
          <Segmented
            label={`${title} presets`}
            value={value}
            options={options}
            onChange={onCommit}
          />
          <NumberField
            className="min-w-0 flex-1"
            label={title}
            value={value}
            unit={unit}
            min={min}
            max={max}
            step={step}
            onCommit={onCommit}
          />
        </ChipField>
      )}
    </SettingChip>
  );
}
