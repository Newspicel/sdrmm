import { Slider as Primitive } from "@base-ui/react/slider";
import type { ReactNode } from "react";

export const SLIDER_CONTROL =
  "flex h-7 w-full cursor-pointer touch-none items-center data-dragging:cursor-grabbing " +
  "data-disabled:cursor-not-allowed data-disabled:opacity-45 pointer-coarse:h-10";

export const SLIDER_TRACK =
  "relative h-2 w-full rounded-inset bg-well " +
  "shadow-[inset_0_0_0_1px_var(--color-line),inset_0_1px_2px_oklch(0_0_0/0.2)]";

export const SLIDER_FILL = "rounded-inset bg-accent";

export const SLIDER_THUMB =
  "h-4 w-1 cursor-grab rounded-inset shadow-[0_0_0_1.5px_var(--color-panel)] " +
  "data-dragging:cursor-grabbing has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-accent " +
  "has-[:focus-visible]:outline-offset-2";

export const SLIDER_THUMB_IDLE = "bg-ink hover:bg-accent";

export function Slider({
  label,
  value,
  min,
  max,
  step,
  onChange,
  onCommit,
  className,
  disabled = false,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  disabled?: boolean;
  onChange: (value: number) => void;
  onCommit?: (value: number) => void;
  className?: string;
}) {
  return (
    <Primitive.Root
      data-hotkeys="off"
      thumbAlignment="edge"
      className={`flex ${className ?? "w-24"}`}
      disabled={disabled}
      value={value}
      min={min}
      max={max}
      step={step}
      onValueChange={(next) => {
        if (typeof next === "number") {
          onChange(next);
        }
      }}
      onValueCommitted={(next) => {
        if (typeof next === "number") {
          onCommit?.(next);
        }
      }}
    >
      <Primitive.Control className={SLIDER_CONTROL}>
        <Primitive.Track className={SLIDER_TRACK}>
          <Primitive.Indicator className={SLIDER_FILL} />
          <Primitive.Thumb aria-label={label} className={`${SLIDER_THUMB} ${SLIDER_THUMB_IDLE}`} />
        </Primitive.Track>
      </Primitive.Control>
    </Primitive.Root>
  );
}

export function SliderField({
  label,
  value,
  min,
  max,
  step,
  readout,
  disabled = false,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  readout: ReactNode;
  disabled?: boolean;
  onChange: (value: number) => void;
}) {
  return (
    <span className="flex min-w-0 flex-1 items-center gap-3">
      <Slider
        label={label}
        className="min-w-0 flex-1"
        disabled={disabled}
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={onChange}
      />
      <span
        className={`w-14 shrink-0 text-right font-mono text-xs tabular-nums ${
          disabled ? "text-ink-faint opacity-45" : "text-ink"
        }`}
      >
        {readout}
      </span>
    </span>
  );
}
