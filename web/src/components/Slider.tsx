import {
  Slider as Primitive,
  SliderControl,
  SliderIndicator,
  SliderThumb,
  SliderTrack,
} from "@ui-sdk/react/slider";
import type { ReactNode } from "react";

export const SLIDER_CONTROL =
  "flex h-7 w-full cursor-pointer touch-none items-center data-dragging:cursor-grabbing " +
  "data-disabled:cursor-not-allowed data-disabled:opacity-45 pointer-coarse:h-10";

export const SLIDER_TRACK = "relative h-2 w-full rounded-inset bg-well inset-shadow-track";

/** Insets the travel by half the thumb, so the thumb stays inside the rail at both ends. */
export const SLIDER_TRAVEL = "mx-0.5 h-full";

/** Starts the fill at the rail's edge and ends it at the thumb's center. */
export const SLIDER_FILL = "-ml-0.5 box-content rounded-inset bg-accent pl-0.5";

export const SLIDER_THUMB =
  "h-4 w-1 cursor-grab rounded-inset shadow-thumb " +
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
    <Primitive
      data-hotkeys="off"
      className={`flex ${className ?? "w-24"}`}
      disabled={disabled}
      value={value}
      min={min}
      max={max}
      step={step}
      onValueChange={(next) => onChange(next)}
      onValueCommit={(next) => onCommit?.(next)}
    >
      <SliderControl className={SLIDER_CONTROL}>
        <div className={SLIDER_TRACK}>
          <SliderTrack className={SLIDER_TRAVEL}>
            <SliderIndicator className={SLIDER_FILL} />
            <SliderThumb aria-label={label} className={`${SLIDER_THUMB} ${SLIDER_THUMB_IDLE}`} />
          </SliderTrack>
        </div>
      </SliderControl>
    </Primitive>
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
  onCommit,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  readout: ReactNode;
  disabled?: boolean;
  onChange: (value: number) => void;
  onCommit?: (value: number) => void;
}) {
  return (
    <span className="flex min-w-0 grow basis-40 items-center gap-3">
      <Slider
        label={label}
        className="min-w-0 flex-1"
        disabled={disabled}
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={onChange}
        onCommit={onCommit}
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
