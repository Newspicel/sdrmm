import {
  Slider as Primitive,
  SliderControl,
  SliderIndicator,
  SliderThumb,
  SliderTrack,
} from "@ui-sdk/react/slider";
import type { ReactNode } from "react";
import { PortAnchor } from "../../canvas/nodes/NodeShell";
import { formatPeak, HEADROOM, type MeterTone, meterUnit } from "../dbfs";
import {
  SLIDER_CONTROL,
  SLIDER_FILL,
  SLIDER_THUMB,
  SLIDER_THUMB_IDLE,
  SLIDER_TRACK,
  SLIDER_TRAVEL,
} from "../Slider";
import { useRows } from "./Chips";

const TONE: Record<MeterTone, string> = {
  ok: "bg-ok",
  warn: "bg-warn",
  danger: "bg-danger",
};

const HEADROOM_TINT =
  `linear-gradient(to right, transparent ${HEADROOM.warn * 100}%, ` +
  `color-mix(in oklab, var(--color-warn) 22%, transparent) ${HEADROOM.warn * 100}% ${HEADROOM.hot * 100}%, ` +
  `color-mix(in oklab, var(--color-danger) 26%, transparent) ${HEADROOM.hot * 100}%)`;

const THUMB_AUTO =
  "bg-panel shadow-[inset_0_0_0_1.5px_var(--color-ink-dim),0_0_0_1.5px_var(--color-panel)] " +
  "hover:shadow-[inset_0_0_0_1.5px_var(--color-accent),0_0_0_1.5px_var(--color-panel)]";

const STACKED_ROW = "grid h-7 grid-cols-[minmax(0,1fr)_3.75rem_2.75rem] items-center gap-x-2";

const STACKED_SOLO = "grid h-7 grid-cols-[minmax(0,1fr)_3.75rem] items-center gap-x-2";

const ROW =
  "grid h-7 grid-cols-[var(--meter-label,3.5rem)_minmax(0,1fr)_3.75rem_2.75rem_0] items-center gap-x-2";

const SOLO_ROW =
  "grid h-7 grid-cols-[var(--meter-label,3.5rem)_minmax(0,1fr)_3.75rem_0] items-center gap-x-2";

export const METER_READOUT = "text-right font-mono text-xs tabular-nums whitespace-nowrap text-ink";

export interface MeterFill {
  level: number;
  peak?: number;
  className: string;
  tint?: string;
}

export function MeterSlider({
  label,
  value,
  min,
  max,
  step,
  fill,
  title,
  auto = false,
  disabled = false,
  onChange,
  onCommit,
  className,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  fill?: MeterFill;
  title?: string;
  auto?: boolean;
  disabled?: boolean;
  onChange: (value: number) => void;
  onCommit?: (value: number) => void;
  className?: string;
}) {
  return (
    <Primitive
      data-hotkeys="off"
      className={`flex ${className ?? "w-24"}`}
      value={value}
      min={min}
      max={max}
      step={step}
      disabled={disabled}
      onValueChange={(next) => onChange(next)}
      onValueCommit={(next) => onCommit?.(next)}
    >
      <SliderControl className={SLIDER_CONTROL} title={title}>
        <div
          className={SLIDER_TRACK}
          style={fill?.tint === undefined ? undefined : { backgroundImage: fill.tint }}
        >
          {fill !== undefined && (
            <>
              <span
                aria-hidden
                className={`absolute inset-y-px left-px rounded-inset ${fill.className}`}
                style={{ width: `max(0px, calc(${fill.level * 100}% - 2px))` }}
              />
              {fill.peak !== undefined && fill.peak > 0 && (
                <span
                  aria-hidden
                  className="absolute inset-y-px w-px bg-ink-dim"
                  style={{ left: `calc(${fill.peak * 100}% - 1px)` }}
                />
              )}
            </>
          )}
          <SliderTrack className={SLIDER_TRAVEL}>
            {fill === undefined && <SliderIndicator className={SLIDER_FILL} />}
            <SliderThumb
              aria-label={label}
              className={`${SLIDER_THUMB} ${auto ? THUMB_AUTO : SLIDER_THUMB_IDLE}`}
            />
          </SliderTrack>
        </div>
      </SliderControl>
    </Primitive>
  );
}

export function MeterBar({
  label,
  value,
  valueText,
  peak,
  fill = "bg-accent",
}: {
  label: string;
  value: number;
  valueText?: string;
  peak?: number;
  fill?: string;
}) {
  const unit = Math.min(1, Math.max(0, value));
  return (
    <span
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(unit * 100)}
      aria-valuetext={valueText}
      className={`block ${SLIDER_TRACK}`}
    >
      <span
        aria-hidden
        className={`absolute inset-y-px left-px rounded-inset ${fill}`}
        style={{ width: `max(0px, calc(${unit * 100}% - 2px))` }}
      />
      {peak !== undefined && peak > 0 && (
        <span
          aria-hidden
          className="absolute inset-y-px w-px bg-ink-dim"
          style={{ left: `calc(${Math.min(1, peak) * 100}% - 1px)` }}
        />
      )}
    </span>
  );
}

export function GainMeter({
  peakDb,
  tone = "ok",
  ...slider
}: Omit<Parameters<typeof MeterSlider>[0], "fill" | "title"> & {
  peakDb?: number | null;
  tone?: MeterTone;
}) {
  const metered = peakDb !== undefined;
  return (
    <MeterSlider
      {...slider}
      fill={
        metered
          ? { level: meterUnit(peakDb ?? undefined), className: TONE[tone], tint: HEADROOM_TINT }
          : undefined
      }
      title={
        metered
          ? `Handle: gain. Fill: signal, ${formatPeak(peakDb ?? undefined)}. Keep it out of the red`
          : undefined
      }
    />
  );
}

export function MeterRow({
  label,
  meter,
  readout,
  trailing,
  port,
  title,
  solo = false,
}: {
  label: ReactNode;
  meter: ReactNode;
  readout?: ReactNode;
  trailing?: ReactNode;
  port?: string;
  title?: string;
  /** The only row on its face: the meter takes the room a trailing button would. */
  solo?: boolean;
}) {
  const end = solo ? null : (trailing ?? <span />);
  if (useRows()) {
    return (
      <div className="flex min-w-0 flex-col gap-1" title={title}>
        {typeof label === "string" ? <span className="text-2xs text-ink-dim">{label}</span> : label}
        <div className={solo ? STACKED_SOLO : STACKED_ROW}>
          {meter}
          {readout === undefined ? <span /> : <span className={METER_READOUT}>{readout}</span>}
          {end}
        </div>
      </div>
    );
  }
  return (
    <div className={solo ? SOLO_ROW : ROW} title={title}>
      {typeof label === "string" ? <span className="legend truncate">{label}</span> : label}
      {meter}
      {readout === undefined ? <span /> : <span className={METER_READOUT}>{readout}</span>}
      {end}
      {port !== undefined && <PortAnchor port={port} />}
    </div>
  );
}
