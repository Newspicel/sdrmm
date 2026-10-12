import { useMemo, useState } from "react";
import { Button } from "../../components/BaseControls";
import { BTN, LABEL } from "../../components/controls";
import { NumberField } from "../../components/NumberField";
import { Segmented } from "../../components/Segmented";
import type { NanoVnaPoint } from "../../lib/types";
import { linearTicks } from "./axis";
import { type Domain, Plot, type PlotTrace } from "./Plot";
import { timeDomain } from "./tdr";
import { LIVE_COLOR, nearestIndex } from "./views";

type TdrShow = "impulse" | "step";

const SHOW_OPTIONS = [
  { value: "impulse", label: "Reflections" },
  { value: "step", label: "Impedance", title: "Needs a low-pass sweep" },
] as const;

const DEFAULT_VELOCITY = 0.66;

export function TdrView({
  points,
  onHarmonicGrid,
}: {
  points: readonly NanoVnaPoint[];
  onHarmonicGrid: () => void;
}) {
  const [velocity, setVelocity] = useState(DEFAULT_VELOCITY);
  const [show, setShow] = useState<TdrShow>("impulse");
  const [zoom, setZoom] = useState<Domain | null>(null);
  const result = useMemo(() => timeDomain(points, velocity), [points, velocity]);
  const step = show === "step" ? result.step : null;
  const values = step ?? result.impulse;
  const full = { low: 0, high: result.rangeM / 2 || 1 };
  const xDomain = zoom ?? full;
  const yDomain = step === null ? impulseDomain(result.lowpass) : stepDomain(values);
  const trace: PlotTrace = {
    key: "tdr",
    label: step === null ? "Reflection" : "Impedance",
    color: LIVE_COLOR,
    dashed: false,
    xs: result.distanceM,
    values,
  };
  const peak = result.peak;

  return (
    <div className="flex h-full min-h-0 flex-col gap-2">
      <div className="flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1">
          <span
            className={LABEL}
            title="Velocity factor of the cable: about 0.66 for solid PE, 0.82 for foam"
          >
            Velocity factor
          </span>
          <NumberField
            label="Velocity factor"
            value={velocity}
            onCommit={setVelocity}
            min={0.1}
            max={1}
            step={0.01}
            className="w-24"
          />
        </label>
        <Segmented
          label="Time domain view"
          value={show}
          options={SHOW_OPTIONS.map((option) => ({
            ...option,
            disabled: option.value === "step" && !result.lowpass,
          }))}
          onChange={setShow}
        />
        {!result.lowpass && (
          <Button
            type="button"
            className={BTN}
            onClick={onHarmonicGrid}
            title="Set start to the point spacing, so the impedance step can be computed"
          >
            Low-pass grid
          </Button>
        )}
        {zoom !== null && (
          <Button type="button" className={BTN} onClick={() => setZoom(null)}>
            Reset zoom
          </Button>
        )}
        <span className="ml-auto font-mono text-xs text-ink-dim tabular-nums">
          {peak === null ? "No reflection found" : `Reflection at ${peak.distanceM.toFixed(2)} m`}
          <span className="text-ink-faint"> · resolution {result.resolutionM.toFixed(2)} m</span>
        </span>
      </div>
      <div className="min-h-0 flex-1">
        <Plot
          label="Reflections against distance"
          xDomain={xDomain}
          yDomain={yDomain}
          xTicks={(domain, width) =>
            linearTicks(domain.low, domain.high, width / 90, (value) => `${trim(value)} m`)
          }
          yTicks={(domain, height) => linearTicks(domain.low, domain.high, height / 50)}
          traces={[trace]}
          markers={
            peak === null ? [] : [{ id: "peak", label: "▼", x: peak.distanceM, active: true }]
          }
          readAt={(x) => {
            const index = nearestIndex(result.distanceM, x);
            const value = values[index];
            const distance = result.distanceM[index];
            return value === undefined || distance === undefined
              ? null
              : {
                  title: `${distance.toFixed(2)} m`,
                  rows: [
                    {
                      key: "tdr",
                      label: trace.label,
                      color: LIVE_COLOR,
                      value: step === null ? value.toFixed(3) : `${value.toFixed(1)} Ω`,
                    },
                  ],
                };
          }}
          onPick={() => undefined}
          onZoom={setZoom}
        />
      </div>
    </div>
  );
}

function impulseDomain(lowpass: boolean): Domain {
  return lowpass ? { low: -1, high: 1 } : { low: 0, high: 1 };
}

function stepDomain(values: readonly number[]): Domain {
  const finite = values.filter(Number.isFinite);
  const high = Math.min(
    500,
    finite.reduce((largest, value) => Math.max(largest, value), 100),
  );
  return { low: 0, high: Math.ceil(high / 25) * 25 };
}

function trim(value: number): string {
  return Number(value.toPrecision(6)).toString();
}
