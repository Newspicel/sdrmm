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
  type Settings,
  shownFields,
} from "../lib/settingsForm";
import type { SettingField } from "../lib/types";
import { Button } from "./BaseControls";
import { Checkbox } from "./Checkbox";
import { BTN_SM } from "./controls";
import { SettingChip } from "./face/Chips";
import { HzField } from "./HzField";
import { Icon } from "./Icon";
import { NumberField, OptionalNumberField } from "./NumberField";
import { rowKeys } from "./rowKeys";
import { Select } from "./Select";
import { TextField } from "./TextField";

export function listShown(entries: readonly unknown[], empty: string | null | undefined): string {
  return entries.length === 0 ? (empty ?? "none") : String(entries.length);
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
  const rows = rowKeys(entries);
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
          key={rows[index]}
          item={item}
          entry={entry}
          name={`${label} ${index + 1}`}
          onEdit={(next) => onChange(entries.with(index, next))}
          onRemove={() => onChange(entries.filter((_, at) => at !== index))}
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
            field={field}
            name={`${name} ${field.label}`}
            value={fieldValue(field, entry)}
            onCommit={(value) => onEdit(committed(item, entry, field.key, value))}
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

function ItemControl({
  field,
  name,
  value,
  onCommit,
}: {
  field: SettingField;
  name: string;
  value: unknown;
  onCommit: (value: unknown) => void;
}) {
  const control = field.control.type === "live" ? field.control.fallback : field.control;
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
      return <ItemNumber name={name} spec={control} value={value} onCommit={onCommit} />;
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
  spec: Extract<SettingField["control"], { type: "number" }>;
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
