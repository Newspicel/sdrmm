import { NumberInput, NumberInputControl } from "@ui-sdk/react/number-input";
import { type KeyboardEvent, useState } from "react";
import { CONTROL_W, FIELD } from "./controls";
import { fractionDigits } from "./format";
import { InFieldUnit, unitPadding } from "./Unit";

interface Common {
  label: string;
  min?: number;
  max?: number;
  step?: number;
  digits?: number;
  className?: string;
  invalid?: boolean;
  disabled?: boolean;
  unit?: string;
}

export function NumberField({
  label,
  value,
  onCommit,
  min,
  max,
  step,
  digits,
  className,
  invalid,
  disabled,
  unit,
}: Common & { value: number; onCommit: (value: number) => void }) {
  const [draft, setDraft] = useDraft(value);
  return (
    <Field
      label={label}
      min={min}
      max={max}
      step={step}
      digits={digits}
      className={className}
      invalid={invalid}
      disabled={disabled}
      unit={unit}
      value={draft}
      onDraft={setDraft}
      onCommit={(committed) => {
        if (committed === null) {
          setDraft(value);
        } else if (committed !== value) {
          onCommit(committed);
        }
      }}
      onRevert={() => setDraft(value)}
    />
  );
}

export function OptionalNumberField({
  label,
  placeholder,
  value,
  onCommit,
  min,
  max,
  step,
  digits,
  className,
  invalid,
  unit,
}: Common & {
  placeholder: string;
  value: number | null;
  onCommit: (value: number | null) => void;
}) {
  const [draft, setDraft] = useDraft(value);
  return (
    <Field
      label={label}
      placeholder={placeholder}
      min={min}
      max={max}
      step={step}
      digits={digits}
      className={className}
      invalid={invalid}
      unit={unit}
      value={draft}
      onDraft={setDraft}
      onCommit={(committed) => {
        if (committed !== value) {
          onCommit(committed);
        }
      }}
      onRevert={() => setDraft(value)}
    />
  );
}

function Field({
  label,
  placeholder,
  min,
  max,
  step,
  digits,
  className,
  invalid,
  disabled,
  unit,
  value,
  onDraft,
  onCommit,
  onRevert,
}: Common & {
  placeholder?: string;
  value: number | null;
  onDraft: (value: number | null) => void;
  onCommit: (value: number | null) => void;
  onRevert: () => void;
}) {
  const shown = digits ?? fractionDigits(step);
  return (
    <NumberInput
      className={`relative ${className ?? CONTROL_W}`}
      value={value}
      min={min}
      max={max}
      step={10 ** -shown}
      largeStep={LARGE_STEP}
      disabled={disabled}
      onValueChange={(next) => onDraft(next)}
      onValueCommit={(next) => onCommit(next)}
      format={{ useGrouping: false, maximumFractionDigits: shown }}
    >
      <NumberInputControl
        aria-label={label}
        aria-invalid={invalid}
        placeholder={placeholder}
        className={`${FIELD} w-full tabular-nums ${invalid === true ? "border-danger" : ""}`}
        style={unit === undefined ? undefined : unitPadding(unit)}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            onRevert();
          }
          const amount = arrowStep(event, step);
          if (amount !== null) {
            event.preventDefault();
            const next = stepped(value, amount, { min, max, digits: shown });
            onDraft(next);
            onCommit(next);
          }
        }}
      />
      {unit !== undefined && <InFieldUnit symbol={unit} />}
    </NumberInput>
  );
}

const LARGE_STEP = 10;
const SMALL_STEP = 0.1;
const ARROWS: Record<string, number> = { ArrowUp: 1, ArrowDown: -1 };

/**
 * The input's own grid is the shown precision, so typed values keep every digit shown;
 * arrows step by the field's `step` here instead.
 */
export function arrowStep(
  event: Pick<KeyboardEvent, "key" | "shiftKey" | "altKey">,
  step: number | undefined,
): number | null {
  const direction = ARROWS[event.key];
  if (direction === undefined) {
    return null;
  }
  const size = event.shiftKey ? LARGE_STEP : event.altKey ? SMALL_STEP : (step ?? 1);
  return direction * size;
}

export function stepped(
  value: number | null,
  amount: number,
  { min, max, digits }: { min?: number; max?: number; digits: number },
): number {
  const next = (value ?? 0) + amount;
  const clamped = Math.min(max ?? Infinity, Math.max(min ?? -Infinity, next));
  return Number(clamped.toFixed(digits));
}

function useDraft(value: number | null): [number | null, (next: number | null) => void] {
  const [draft, setDraft] = useState(value);
  const [seen, setSeen] = useState(value);
  if (seen !== value) {
    setSeen(value);
    setDraft(value);
    return [value, setDraft];
  }
  return [draft, setDraft];
}
