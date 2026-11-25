import type { ReactNode } from "react";
import { Button } from "../../components/BaseControls";
import { KEY } from "../../components/controls";
import { DB_LIMIT, DB_STEP, withCeiling, withFloor } from "../../components/dbRange";
import { Chips, ChoiceChip, ToggleChip } from "../../components/face/Chips";
import { GradientEditor } from "../../components/GradientEditor";
import { Swatch } from "../../components/PlotSettings";
import { Slider } from "../../components/Slider";
import { type DbWindow, TRACE_MODES, type TraceMode } from "../../components/spectrumTraces";
import { AVERAGE_CHOICES, type AverageFrames } from "../../components/videoAverage";
import { COLORMAPS, type Palette, type Rgb, samplePalette } from "../../gl/colormap";
import { SPECTRUM_OVERLAPS, type SpectrumOverlap } from "../../lib/spectrum";
import { Inspect, InspectSection } from "../inspector";
import { CUSTOM, type PaletteChoice, paletteOf } from "./scopePalette";
import { TRACE_INK } from "./scopePlot";

const GRADIENT_STOPS = 8;

const TRACE_LABEL: Record<TraceMode, string> = {
  peak: "peak hold",
  average: "average",
  min: "min hold",
};

const AVERAGE_OPTIONS = AVERAGE_CHOICES.map((frames) => ({
  value: frames,
  label: frames === 1 ? "off" : String(frames),
}));

const OVERLAP_OPTIONS = SPECTRUM_OVERLAPS.map((overlap) => ({ value: overlap, label: overlap }));

export interface ScopeSettingsProps {
  node: string;
  colormap: PaletteChoice;
  onColormap: (choice: PaletteChoice) => void;
  custom: readonly Rgb[];
  onCustom: (stops: readonly Rgb[]) => void;
  average: AverageFrames;
  onAverage: (frames: AverageFrames) => void;
  overlap: SpectrumOverlap;
  onOverlap: (overlap: SpectrumOverlap) => void;
  traces: readonly TraceMode[];
  onTrace: (mode: TraceMode) => void;
  phosphor: boolean;
  onPhosphor: () => void;
  bands: boolean;
  onBands: () => void;
  range: DbWindow;
  manual: boolean;
  onRange: (range: DbWindow) => void;
  onAuto: () => void;
}

export function ScopeSettings(props: ScopeSettingsProps) {
  return (
    <Inspect node={props.node}>
      <InspectSection label="Display">
        <Chips inset="none">
          <ChoiceChip
            label="Average"
            title="Frames blended into the trace and waterfall"
            value={props.average}
            options={AVERAGE_OPTIONS}
            onChange={props.onAverage}
          />
          <ChoiceChip
            label="Overlap"
            title="Smoother trace, more server CPU. The highest open Scope wins"
            value={props.overlap}
            options={OVERLAP_OPTIONS}
            onChange={props.onOverlap}
          />
          <ToggleChip
            label="Band plan"
            title="Show band allocations above the trace"
            on={props.bands}
            onChange={props.onBands}
          />
        </Chips>
      </InspectSection>
      <InspectSection label="Colours">
        <Colours {...props} />
      </InspectSection>
      <InspectSection label="Traces">
        <Traces {...props} />
      </InspectSection>
      <InspectSection label="Levels">
        <Levels {...props} />
      </InspectSection>
    </Inspect>
  );
}

function Colours(props: ScopeSettingsProps) {
  return (
    <>
      <div className="grid grid-cols-3 gap-1.5">
        {COLORMAPS.map((name) => (
          <Swatch
            key={name}
            name={name}
            fill={gradient(name, "to right")}
            on={name === props.colormap}
            onClick={() => props.onColormap(name)}
          />
        ))}
        <Swatch
          name={CUSTOM}
          fill={gradient(props.custom, "to right")}
          on={props.colormap === CUSTOM}
          onClick={() => props.onColormap(CUSTOM)}
        />
      </div>
      {props.colormap === CUSTOM && (
        <GradientEditor stops={props.custom} onChange={props.onCustom} />
      )}
    </>
  );
}

function Traces(props: ScopeSettingsProps) {
  const palette = paletteOf(props.colormap, props.custom);
  return (
    <div className="grid grid-cols-2 gap-1.5">
      {TRACE_MODES.map((mode) => (
        <TraceToggle
          key={mode}
          label={TRACE_LABEL[mode]}
          on={props.traces.includes(mode)}
          onClick={() => props.onTrace(mode)}
        >
          <TraceSample>
            <span
              className="h-0.5 w-full rounded-full"
              style={{ background: `var(--color-${TRACE_INK[mode]})` }}
            />
          </TraceSample>
        </TraceToggle>
      ))}
      <TraceToggle label="phosphor" on={props.phosphor} onClick={props.onPhosphor}>
        <TraceSample>
          <span
            className="-mx-0.5 h-full w-[calc(100%+4px)]"
            style={{ background: gradient(palette, "to top") }}
          />
        </TraceSample>
      </TraceToggle>
    </div>
  );
}

function Levels(props: ScopeSettingsProps) {
  return (
    <>
      <Chips inset="none">
        <ToggleChip
          label="Auto"
          title="Fit the waterfall colours to the signal"
          on={!props.manual}
          onChange={(auto) => (auto ? props.onAuto() : props.onRange(props.range))}
        />
      </Chips>
      <Level
        name="floor"
        label="Waterfall dBFS floor"
        value={props.range.min}
        onChange={(db) => props.onRange(withFloor(props.range, db))}
      />
      <Level
        name="ceiling"
        label="Waterfall dBFS ceiling"
        value={props.range.max}
        onChange={(db) => props.onRange(withCeiling(props.range, db))}
      />
    </>
  );
}

function gradient(palette: Palette, direction: string): string {
  const stops = Array.from({ length: GRADIENT_STOPS }, (_, index) => {
    const [r, g, b] = samplePalette(palette, index / (GRADIENT_STOPS - 1));
    return `rgb(${Math.round(r * 255)} ${Math.round(g * 255)} ${Math.round(b * 255)})`;
  });
  return `linear-gradient(${direction}, ${stops.join(", ")})`;
}

function TraceSample({ children }: { children: ReactNode }) {
  return (
    <span className="flex h-3.5 w-5 shrink-0 items-center overflow-hidden rounded-control bg-plot-bg px-0.5">
      {children}
    </span>
  );
}

function Level({
  name,
  label,
  value,
  onChange,
}: {
  name: string;
  label: string;
  value: number;
  onChange: (db: number) => void;
}) {
  return (
    <div className="flex items-center gap-3">
      <span className="w-12 shrink-0 font-mono text-2xs text-ink-faint">{name}</span>
      <Slider
        label={label}
        className="min-w-0 flex-1"
        min={DB_LIMIT.min}
        max={DB_LIMIT.max}
        step={DB_STEP}
        value={value}
        onChange={onChange}
      />
      <span className="w-14 shrink-0 text-right font-mono text-2xs tabular-nums text-ink-dim">
        {value} dB
      </span>
    </div>
  );
}

function TraceToggle({
  label,
  on,
  onClick,
  children,
}: {
  label: string;
  on: boolean;
  onClick: () => void;
  children?: ReactNode;
}) {
  return (
    <Button type="button" aria-pressed={on} onClick={onClick} className={KEY}>
      {children}
      {label}
    </Button>
  );
}
