import { Plus, X } from "lucide-react";
import { useState } from "react";
import {
  blankEntry,
  choiceItems,
  chosenValue,
  committed,
  entryComplete,
  type Form,
  fieldValue,
  isQuiet,
  listEntries,
  presetIndex,
  type Settings,
  shownFields,
  shownNumber,
  shownSpec,
  storedNumber,
  withPreset,
} from "../lib/settingsForm";
import type { Control, NumberSpec, PresetOption, SettingField } from "../lib/types";
import { Button } from "./BaseControls";
import { Checkbox } from "./Checkbox";
import { BTN_SM, type Options } from "./controls";
import { SettingChip } from "./face/Chips";
import { HzField } from "./HzField";
import { Icon } from "./Icon";
import { NumberField, OptionalNumberField } from "./NumberField";
import { Select } from "./Select";
import { TextField } from "./TextField";

export function listShown(entries: readonly unknown[], empty: string | null | undefined): string {
  return entries.length === 0 ? (empty ?? "none") : String(entries.length);
}

let entrySerial = 0;

function freshId(): string {
  entrySerial += 1;
  return `entry-${entrySerial}`;
}

/** `ids` cut or grown to `count`, keeping the ids it has so their rows stay mounted. */
export function fitIds(ids: readonly string[], count: number): string[] {
  const kept = ids.slice(0, count);
  return [...kept, ...Array.from({ length: count - kept.length }, freshId)];
}

/** Row ids that follow entries through edits and removals, so a nested control stays open. */
function useEntryIds(count: number): { ids: readonly string[]; removeAt: (index: number) => void } {
  const [ids, setIds] = useState<readonly string[]>(() => fitIds([], count));
  const fitted = ids.length === count ? ids : fitIds(ids, count);
  if (fitted !== ids) {
    setIds(fitted);
  }
  return {
    ids: fitted,
    removeAt: (index) => setIds((current) => current.filter((_, at) => at !== index)),
  };
}

export function ListChip({
  label,
  title,
  item,
  entries,
  empty,
  quiet,
  onChange,
}: {
  label: string;
  title: string;
  item: Form;
  entries: readonly Settings[];
  empty?: string | null;
  quiet: boolean;
  onChange: (entries: Settings[]) => void;
}) {
  return (
    <SettingChip
      label={label}
      value={listShown(entries, empty)}
      quiet={quiet}
      title={title}
      width="w-96"
    >
      {() => <ListEditor label={label} item={item} entries={entries} onChange={onChange} />}
    </SettingChip>
  );
}

function ListEditor({
  label,
  item,
  entries,
  onChange,
}: {
  label: string;
  item: Form;
  entries: readonly Settings[];
  onChange: (entries: Settings[]) => void;
}) {
  const rows = useEntryIds(entries.length);
  const [draft, setDraft] = useState<Settings | null>(null);
  const keep = (next: Settings): void => {
    if (entryComplete(item, next)) {
      onChange([...entries, next]);
      setDraft(null);
    } else {
      setDraft(next);
    }
  };
  return (
    <div className="flex flex-col gap-2">
      {entries.map((entry, index) => (
        <EntryRow
          key={rows.ids[index]}
          item={item}
          entry={entry}
          name={`${label} ${index + 1}`}
          onEdit={(next) => onChange(entries.with(index, next))}
          onRemove={() => {
            rows.removeAt(index);
            onChange(entries.filter((_, at) => at !== index));
          }}
        />
      ))}
      {draft !== null && (
        <EntryRow
          item={item}
          entry={draft}
          name={`New ${label.toLowerCase()}`}
          onEdit={keep}
          onRemove={() => setDraft(null)}
        />
      )}
      <Button
        type="button"
        className={`${BTN_SM} self-start`}
        disabled={draft !== null}
        onClick={() => keep(blankEntry(item))}
      >
        <Icon glyph={Plus} size={12} />
        Add
      </Button>
    </div>
  );
}

function EntryRow({
  item,
  entry,
  name,
  onEdit,
  onRemove,
}: {
  item: Form;
  entry: Settings;
  name: string;
  onEdit: (next: Settings) => void;
  onRemove: () => void;
}) {
  return (
    <div className="flex items-center gap-1">
      <div className="flex min-w-0 flex-1 flex-wrap items-center gap-1">
        {shownFields(item, entry).map((field) => (
          <ItemControl
            key={`${field.key}:${field.label}`}
            item={item}
            entry={entry}
            field={field}
            name={`${name} ${field.label}`}
            onEdit={onEdit}
          />
        ))}
      </div>
      <Button
        type="button"
        className={`${BTN_SM} hover:text-danger`}
        aria-label={`Remove ${name.toLowerCase()}`}
        onClick={onRemove}
      >
        <Icon glyph={X} size={12} />
      </Button>
    </div>
  );
}

const ITEM = "min-w-0 flex-1 basis-24";

/** One field of a list entry. Presets and nested lists edit the whole entry. */
export function ItemControl({
  item,
  entry,
  field,
  name,
  onEdit,
}: {
  item: Form;
  entry: Settings;
  field: SettingField;
  name: string;
  onEdit: (next: Settings) => void;
}) {
  const control = field.control.type === "live" ? field.control.fallback : field.control;
  const value = fieldValue(field, entry);
  const onCommit = (next: unknown): void => onEdit(committed(item, entry, field.key, next));
  switch (control.type) {
    case "preset":
      return (
        <ItemPreset
          name={name}
          options={control.options}
          chosen={presetIndex(item, entry, control.options)}
          onPick={(option) => onEdit(withPreset(item, entry, option))}
        />
      );
    case "list":
      return (
        <ListChip
          label={field.label}
          title={field.title ?? field.label}
          item={control.item}
          entries={listEntries(value)}
          empty={control.empty}
          quiet={isQuiet(field, value)}
          onChange={onCommit}
        />
      );
    default:
      return (
        <ValueControl
          field={field}
          control={control}
          name={name}
          value={value}
          onCommit={onCommit}
        />
      );
  }
}

const CUSTOM = -1;

function ItemPreset({
  name,
  options,
  chosen,
  onPick,
}: {
  name: string;
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
    ...(chosen === CUSTOM ? [{ value: CUSTOM, label: "Custom", disabled: true }] : []),
  ];
  return (
    <Select
      label={name}
      className={ITEM}
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

function ValueControl({
  field,
  control,
  name,
  value,
  onCommit,
}: {
  field: SettingField;
  control: Control;
  name: string;
  value: unknown;
  onCommit: (value: unknown) => void;
}) {
  switch (control.type) {
    case "choice": {
      const { options, chosen } = choiceItems(control.options, value);
      return (
        <Select
          label={name}
          className={ITEM}
          value={chosen}
          options={options}
          onChange={(key) => onCommit(chosenValue(control.options, key, value))}
        />
      );
    }
    case "number":
      return (
        <ItemNumber
          name={name}
          spec={shownSpec(control)}
          value={shownNumber(control, value)}
          onCommit={(shown) => onCommit(storedNumber(control, shown))}
        />
      );
    case "toggle":
      return <Checkbox label={name} checked={value === true} onChange={onCommit} />;
    case "text":
      return (
        <TextField
          label={name}
          className={ITEM}
          value={typeof value === "string" ? value : ""}
          placeholder={control.placeholder ?? field.label}
          secret={control.secret === true}
          maxLength={control.max_len ?? undefined}
          onCommit={(text) => onCommit(text === "" && control.optional === true ? null : text)}
        />
      );
    default:
      return null;
  }
}

function ItemNumber({
  name,
  spec,
  value,
  onCommit,
}: {
  name: string;
  spec: NumberSpec;
  value: unknown;
  onCommit: (value: number | null) => void;
}) {
  const number = typeof value === "number" ? value : null;
  const limits = { min: spec.min ?? undefined, max: spec.max ?? undefined };
  if (spec.unit === "Hz") {
    return (
      <HzField
        label={name}
        className={ITEM}
        value={number}
        placeholder={spec.optional ?? undefined}
        optional={spec.optional != null}
        {...limits}
        onCommit={onCommit}
      />
    );
  }
  if (spec.optional != null || number === null) {
    return (
      <OptionalNumberField
        label={name}
        className={ITEM}
        placeholder={spec.optional ?? name}
        value={number}
        unit={spec.unit ?? undefined}
        step={spec.step ?? undefined}
        {...limits}
        onCommit={onCommit}
      />
    );
  }
  return (
    <NumberField
      label={name}
      className={ITEM}
      value={number}
      unit={spec.unit ?? undefined}
      step={spec.step ?? undefined}
      {...limits}
      onCommit={onCommit}
    />
  );
}
