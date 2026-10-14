import { useState } from "react";
import type { ExtraSetting, Range } from "../lib/types";
import { Input } from "./BaseControls";
import { Checkbox } from "./Checkbox";
import { fitsSlider } from "./capabilities";
import { FIELD } from "./controls";
import { ChipField, SettingChip, ToggleChip } from "./face/Chips";
import { NumberField } from "./NumberField";
import { SearchableSelect } from "./SearchableSelect";
import { Select } from "./Select";
import { Slider } from "./Slider";
import { settingLabel } from "./settingLabel";
import { Unit } from "./Unit";
import { useDebouncedCommit } from "./useDebouncedCommit";

const SEARCHABLE_FROM = 12;

const WIDE = "min-w-0 flex-1";

type Value = boolean | string | number;

type Held = readonly { name: string; value: unknown }[];

type Entry = { group: string | null; members: ExtraSetting[] };

export function ExtraChips({
  settings,
  values,
  onCommit,
}: {
  settings: readonly ExtraSetting[];
  values: Held;
  onCommit: (name: string, value: Value) => void;
}) {
  const raw = (name: string) => values.find((value) => value.name === name)?.value;
  return (
    <>
      {grouped(settings).map((entry) =>
        entry.group === null ? (
          entry.members.map((setting) => (
            <ExtraChip
              key={setting.name}
              setting={setting}
              raw={raw(setting.name)}
              onCommit={(value) => onCommit(setting.name, value)}
            />
          ))
        ) : (
          <GroupChip
            key={entry.group}
            group={entry.group}
            members={entry.members}
            raw={raw}
            onCommit={onCommit}
          />
        ),
      )}
    </>
  );
}

export function grouped(settings: readonly ExtraSetting[]): Entry[] {
  const entries: Entry[] = [];
  for (const setting of settings) {
    const group = setting.group ?? null;
    const held = group === null ? undefined : entries.find((entry) => entry.group === group);
    if (held === undefined) entries.push({ group, members: [setting] });
    else held.members.push(setting);
  }
  return entries;
}

export function groupSummary(
  members: readonly ExtraSetting[],
  raw: (name: string) => unknown,
): string {
  const switches = members.filter((setting) => setting.kind === "bool");
  if (switches.length > 0) {
    const on = switches.filter((setting) => boolOf(setting, raw(setting.name))).length;
    if (on === 0) return "off";
    if (on === switches.length) return "on";
    return `${on}/${switches.length}`;
  }
  const first = members[0];
  return first === undefined ? "" : shown(first, raw(first.name));
}

function GroupChip({
  group,
  members,
  raw,
  onCommit,
}: {
  group: string;
  members: readonly ExtraSetting[];
  raw: (name: string) => unknown;
  onCommit: (name: string, value: Value) => void;
}) {
  const summary = groupSummary(members, raw);
  return (
    <SettingChip label={group} value={summary} quiet={summary === "off"} title={group}>
      {(close) => (
        <div className="flex flex-col gap-3">
          {members.map((setting) => (
            <ExtraField
              key={setting.name}
              setting={setting}
              raw={raw(setting.name)}
              onCommit={(value) => onCommit(setting.name, value)}
              onDone={close}
            />
          ))}
        </div>
      )}
    </SettingChip>
  );
}

function nameOf(setting: ExtraSetting): string {
  return setting.label ?? settingLabel(setting.name);
}

function boolOf(setting: ExtraSetting & { kind: "bool" }, raw: unknown): boolean {
  return typeof raw === "boolean" ? raw : setting.default;
}

function shown(setting: ExtraSetting, raw: unknown): string {
  switch (setting.kind) {
    case "bool":
      return boolOf(setting, raw) ? "on" : "off";
    case "enum": {
      const value = typeof raw === "string" ? raw : setting.default;
      return setting.options.find((option) => option.value === value)?.label ?? value;
    }
    case "range":
      return String(typeof raw === "number" ? raw : setting.range.min);
    case "string": {
      const value = typeof raw === "string" ? raw : setting.default;
      return value === "" ? "none" : value;
    }
  }
}

function ExtraChip({
  setting,
  raw,
  onCommit,
}: {
  setting: ExtraSetting;
  raw: unknown;
  onCommit: (value: Value) => void;
}) {
  const name = nameOf(setting);
  if (setting.kind === "bool") {
    return <ToggleChip label={name} on={boolOf(setting, raw)} title={name} onChange={onCommit} />;
  }
  const value = shown(setting, raw);
  return (
    <SettingChip
      label={name}
      value={value}
      unit={setting.kind === "range" && setting.unit !== "" ? setting.unit : undefined}
      quiet={setting.kind === "string" && value === "none"}
      title={
        setting.kind === "range" ? `${name}: ${setting.range.min} to ${setting.range.max}` : name
      }
    >
      {(close) => <ExtraField setting={setting} raw={raw} onCommit={onCommit} onDone={close} />}
    </SettingChip>
  );
}

function ExtraField({
  setting,
  raw,
  onCommit,
  onDone,
}: {
  setting: ExtraSetting;
  raw: unknown;
  onCommit: (value: Value) => void;
  onDone: () => void;
}) {
  const name = nameOf(setting);
  switch (setting.kind) {
    case "bool":
      return (
        <label className="flex items-center gap-1.5">
          <Checkbox label={name} checked={boolOf(setting, raw)} onChange={onCommit} />
          <span className="legend">{name}</span>
        </label>
      );
    case "enum": {
      const options = setting.options.map((option) => ({
        value: option.value,
        label: option.label ?? option.value,
      }));
      const Picker = options.length > SEARCHABLE_FROM ? SearchableSelect : Select;
      return (
        <ChipField label={name}>
          <Picker
            className={WIDE}
            label={name}
            value={typeof raw === "string" ? raw : setting.default}
            options={options}
            onChange={onCommit}
          />
        </ChipField>
      );
    }
    case "range": {
      const value = typeof raw === "number" ? raw : setting.range.min;
      return fitsSlider(setting.range) ? (
        <RangeSlider
          name={name}
          unit={setting.unit}
          range={setting.range}
          value={value}
          onCommit={onCommit}
        />
      ) : (
        <ChipField label={name}>
          <NumberField
            className={WIDE}
            label={name}
            unit={setting.unit === "" ? undefined : setting.unit}
            value={value}
            min={setting.range.min}
            max={setting.range.max}
            step={setting.range.step ?? undefined}
            onCommit={onCommit}
          />
        </ChipField>
      );
    }
    case "string":
      return (
        <ChipField label={name}>
          <TextEntry
            label={name}
            value={typeof raw === "string" ? raw : setting.default}
            onCommit={onCommit}
            onDone={onDone}
          />
        </ChipField>
      );
  }
}

function TextEntry({
  label,
  value,
  onCommit,
  onDone,
}: {
  label: string;
  value: string;
  onCommit: (value: string) => void;
  onDone: () => void;
}) {
  const [draft, setDraft] = useState(value);
  return (
    <Input
      autoFocus
      aria-label={label}
      className={`${FIELD} ${WIDE}`}
      value={draft}
      onChange={(event) => setDraft(event.currentTarget.value)}
      onBlur={() => onCommit(draft)}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          onCommit(draft);
          onDone();
        }
      }}
    />
  );
}

function RangeSlider({
  name,
  unit,
  range,
  value,
  onCommit,
}: {
  name: string;
  unit: string;
  range: Range;
  value: number;
  onCommit: (value: number) => void;
}) {
  const { pending, change } = useDebouncedCommit(onCommit);
  const current = pending ?? value;
  const digits = range.step != null && range.step < 1 ? 1 : 0;
  return (
    <ChipField label={name}>
      <Slider
        label={`${name} (${unit})`}
        className="min-w-0 flex-1"
        min={range.min}
        max={range.max}
        step={range.step ?? 1}
        value={current}
        onChange={change}
      />
      <span className="w-14 shrink-0 text-right font-mono text-xs tabular-nums text-ink">
        {current.toFixed(digits)} <Unit symbol={unit} className="text-ink-faint" />
      </span>
    </ChipField>
  );
}
