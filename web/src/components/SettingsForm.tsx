import {
  choiceItems,
  chosenValue,
  committed,
  effectiveControl,
  type Form,
  fieldShown,
  fieldValue,
  isQuiet,
  listEntries,
  presetIndex,
  type Settings,
  withPreset,
} from "../lib/settingsForm";
import type {
  NumberSpec,
  ParamChoice,
  ParamOption,
  PresetOption,
  SettingField,
} from "../lib/types";
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
import { ListChip } from "./ListChip";
import { NumberField } from "./NumberField";
import { rowKeys } from "./rowKeys";
import { Segmented } from "./Segmented";

/** A decoder's settings as chips, drawn from the form its descriptor publishes. */
export function SettingsForm({
  form,
  settings,
  choices,
  onSettings,
}: {
  form: Form;
  settings: Settings;
  choices: readonly ParamChoice[];
  onSettings: (settings: Settings) => void;
}) {
  const keys = rowKeys(form);
  return form.map((field, index) =>
    fieldShown(form, settings, field) ? (
      <FieldChip
        key={keys[index]}
        form={form}
        field={field}
        settings={settings}
        choices={choices}
        onSettings={onSettings}
      />
    ) : null,
  );
}

interface FieldProps {
  form: Form;
  field: SettingField;
  settings: Settings;
  choices: readonly ParamChoice[];
  onSettings: (settings: Settings) => void;
}

function FieldChip({ form, field, settings, choices, onSettings }: FieldProps) {
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
      return <OptionChip {...shared} options={control.options} value={value} onCommit={commit} />;
    case "number":
      return <SpecChip {...shared} spec={control} value={value} onCommit={commit} />;
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
  onCommit,
  ...chip
}: ChipProps & {
  options: readonly ParamOption[];
  value: unknown;
  onCommit: (value: unknown) => void;
}) {
  const items = choiceItems(options, value);
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
