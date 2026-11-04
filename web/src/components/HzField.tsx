import { useState } from "react";
import { inRange, parseHz } from "../lib/settingsForm";
import { Input } from "./BaseControls";
import { CONTROL_W, FIELD } from "./controls";
import { ChipField, SettingChip } from "./face/Chips";
import { formatHz } from "./format";

interface HzProps {
  label: string;
  value: number | null;
  placeholder?: string;
  optional?: boolean;
  min?: number | null;
  max?: number | null;
  onCommit: (hz: number | null) => void;
}

/** The hertz `text` names, or undefined when it names none the field accepts. */
export function enteredHz(
  text: string,
  { optional = false, min, max }: Pick<HzProps, "optional" | "min" | "max">,
): number | null | undefined {
  if (text.trim() === "") {
    return optional ? null : undefined;
  }
  const hz = parseHz(text);
  return hz !== null && inRange(hz, min, max) ? hz : undefined;
}

function hzText(value: number | null): string {
  return value === null ? "" : formatHz(value);
}

/** A frequency typed with or without its prefix: `12500`, `2.4 kHz`, `145.5 MHz`. */
export function HzField({ className = CONTROL_W, ...props }: HzProps & { className?: string }) {
  const { label, value, placeholder, onCommit } = props;
  const [draft, setDraft] = useState(hzText(value));
  const [seen, setSeen] = useState(value);
  if (seen !== value) {
    setSeen(value);
    setDraft(hzText(value));
  }
  const commit = (): void => {
    const hz = enteredHz(draft, props);
    if (hz === undefined || hz === value) {
      setDraft(hzText(value));
      return;
    }
    onCommit(hz);
  };
  return (
    <Input
      className={`${FIELD} tabular-nums ${className}`}
      aria-label={label}
      autoComplete="off"
      placeholder={placeholder}
      value={draft}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={commit}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          event.currentTarget.blur();
        }
        if (event.key === "Escape") {
          setDraft(hzText(value));
        }
      }}
    />
  );
}

export function HzChip({ title, quiet, ...props }: HzProps & { title: string; quiet?: boolean }) {
  const { label, value, placeholder } = props;
  return (
    <SettingChip
      label={label}
      value={value === null ? (placeholder ?? "none") : formatHz(value)}
      title={title}
      quiet={quiet ?? value === null}
      width="w-52"
    >
      {() => (
        <ChipField label={title}>
          <HzField {...props} label={title} className="min-w-0 flex-1" />
        </ChipField>
      )}
    </SettingChip>
  );
}
