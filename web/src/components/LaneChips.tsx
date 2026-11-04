import { X } from "lucide-react";
import { useState } from "react";
import { lanesOf, lanesValue } from "../lib/settingsForm";
import type { LanePick, NumberSpec } from "../lib/types";
import { Button } from "./BaseControls";
import { BTN_SM, type Options, segment, WELL } from "./controls";
import { ChipField, ChoiceChip, SettingChip } from "./face/Chips";
import { Icon } from "./Icon";
import { NumberField } from "./NumberField";

const MIN_LANES = 2;

interface ChipProps {
  label: string;
  title: string;
  quiet: boolean;
}

/** How many lanes to offer: the array's, and never fewer than a setting already holds. */
export function laneCount(lanes: number, held: readonly number[]): number {
  return Math.max(lanes, MIN_LANES, ...held.map((lane) => lane + 1));
}

export function laneOptions(count: number, except: number | null): Options<number> {
  return Array.from({ length: count }, (_, lane) => ({
    value: lane,
    label: String(lane + 1),
    ...(lane === except ? { disabled: true, title: "Taken" } : {}),
  }));
}

export function LanesChip({
  pick,
  value,
  lanes,
  except,
  empty = "none",
  onCommit,
  ...chip
}: ChipProps & {
  pick: LanePick;
  value: unknown;
  lanes: number;
  except: number | null;
  empty?: string;
  onCommit: (value: unknown) => void;
}) {
  const held = lanesOf(pick, value);
  const options = laneOptions(laneCount(lanes, held), except);
  if (pick === "one") {
    return (
      <ChoiceChip
        {...chip}
        value={held[0] ?? 0}
        options={options}
        onChange={(lane) => onCommit(lane)}
      />
    );
  }
  const toggle = (lane: number): void =>
    onCommit(
      lanesValue(
        pick,
        held.includes(lane) ? held.filter((kept) => kept !== lane) : [...held, lane],
      ),
    );
  return (
    <SettingChip
      {...chip}
      value={held.length === 0 ? empty : held.map((lane) => lane + 1).join(" ")}
    >
      {() => (
        <span className={WELL}>
          {options.map((option) => (
            <Button
              key={option.value}
              type="button"
              aria-pressed={held.includes(option.value)}
              aria-label={`${chip.label} ${option.label}`}
              disabled={option.disabled === true}
              title={option.title}
              className={segment(held.includes(option.value))}
              onClick={() => toggle(option.value)}
            >
              {option.label}
            </Button>
          ))}
        </span>
      )}
    </SettingChip>
  );
}

function numbersOf(value: unknown): number[] {
  return Array.isArray(value)
    ? value.filter((entry): entry is number => typeof entry === "number")
    : [];
}

export function NumbersChip({
  spec,
  maxLen,
  value,
  onCommit,
  ...chip
}: ChipProps & {
  spec: NumberSpec;
  maxLen: number;
  value: unknown;
  onCommit: (value: number[]) => void;
}) {
  const held = numbersOf(value);
  const unit = spec.unit ?? "";
  return (
    <SettingChip
      {...chip}
      value={held.length === 0 ? "none" : held.map((entry) => `${entry}${unit}`).join(" ")}
      width="w-72"
    >
      {() => (
        <NumbersEditor
          label={chip.label}
          spec={spec}
          maxLen={maxLen}
          held={held}
          onCommit={onCommit}
        />
      )}
    </SettingChip>
  );
}

function NumbersEditor({
  label,
  spec,
  maxLen,
  held,
  onCommit,
}: {
  label: string;
  spec: NumberSpec;
  maxLen: number;
  held: readonly number[];
  onCommit: (value: number[]) => void;
}) {
  const [draft, setDraft] = useState(spec.min ?? 0);
  const full = held.length >= maxLen;
  return (
    <div className="flex flex-col gap-2">
      {held.length > 0 && (
        <span className="flex flex-wrap gap-2">
          {held.map((entry) => (
            <span key={entry} className="inline-flex items-center gap-1 font-mono text-xs">
              {entry}
              {spec.unit}
              <Button
                type="button"
                aria-label={`Remove ${label.toLowerCase()} ${entry}`}
                className="text-ink-faint hover:text-danger"
                onClick={() => onCommit(held.filter((kept) => kept !== entry))}
              >
                <Icon glyph={X} size={12} />
              </Button>
            </span>
          ))}
        </span>
      )}
      <ChipField label={`New ${label.toLowerCase()}`}>
        <NumberField
          className="min-w-0 flex-1"
          label={`New ${label.toLowerCase()}`}
          value={draft}
          min={spec.min ?? undefined}
          max={spec.max ?? undefined}
          step={spec.step ?? undefined}
          unit={spec.unit ?? undefined}
          disabled={full}
          onCommit={setDraft}
        />
        <Button
          type="button"
          className={BTN_SM}
          disabled={full}
          title={full ? `At most ${maxLen}` : undefined}
          onClick={() => onCommit([...new Set([...held, draft])].toSorted((a, b) => a - b))}
        >
          Add
        </Button>
      </ChipField>
    </div>
  );
}
