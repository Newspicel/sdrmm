import { useMemo, useState } from "react";
import { Button } from "../../components/BaseControls";
import { BTN, LABEL } from "../../components/controls";
import { formatHz } from "../../components/format";
import { Icon } from "../../components/Icon";
import { Segmented } from "../../components/Segmented";
import type { AntennaSimulation } from "../../lib/types";
import { type PointReadout, readouts } from "../nanovna/analysis";
import { bandFits, type HamBand } from "../nanovna/bands";
import { FrequencyChart, Legend } from "../nanovna/MeasureView";
import type { MarkerRow } from "../nanovna/markers";
import type { Domain } from "../nanovna/Plot";
import { BandTable, formatSwr, matchQuality } from "../nanovna/Readouts";
import { SmithChart } from "../nanovna/SmithChart";
import { LIVE_COLOR, nearestIndex, REFERENCE_COLORS, type Subject } from "../nanovna/views";
import { PatternChart } from "./PatternChart";
import { type Matching, pointsOf, roundedHz, swrOf, swrSpan } from "./performance";

type ChartId = "swr" | "s11" | "impedance";

const CHARTS = [
  { value: "swr", label: "SWR" },
  { value: "s11", label: "Return loss" },
  { value: "impedance", label: "Impedance" },
] as const;

const FEEDPOINT_COLOR = REFERENCE_COLORS[0];

export function sweepSubjects(simulation: AntennaSimulation, matching: Matching | null): Subject[] {
  const feedpoint: Subject = {
    key: "feedpoint",
    label: "At the feedpoint",
    color: FEEDPOINT_COLOR,
    dashed: true,
    rows: readouts(pointsOf(simulation.sweep)),
  };
  if (matching === null || matching.id === "direct") {
    return [{ ...feedpoint, color: LIVE_COLOR, dashed: false, label: "Direct" }];
  }
  return [
    {
      key: "matched",
      label: `With ${matching.label}`,
      color: LIVE_COLOR,
      dashed: false,
      rows: readouts(pointsOf(simulation.sweep, matching.apply)),
    },
    feedpoint,
  ];
}

export function centerMarker(rows: readonly PointReadout[], frequencyHz: number): MarkerRow | null {
  const row =
    rows[
      nearestIndex(
        rows.map((entry) => entry.frequencyHz),
        frequencyHz,
      )
    ];
  return row === undefined ? null : { id: "best", label: "f", row };
}

export function PerformanceView({
  simulation,
  frequencyHz,
  grounded,
  bands,
  matchingList,
  matching,
  onMatching,
}: {
  simulation: AntennaSimulation;
  frequencyHz: number;
  grounded: boolean;
  bands: readonly HamBand[];
  matchingList: readonly Matching[];
  matching: Matching | null;
  onMatching: (id: string) => void;
}) {
  const [chart, setChart] = useState<ChartId>("swr");
  const [zoomed, setZoomed] = useState<{ key: string; domain: Domain } | null>(null);
  const subjects = useMemo(() => sweepSubjects(simulation, matching), [simulation, matching]);
  const shown = chart === "impedance" ? subjects.slice(-1) : subjects;
  const rows = useMemo(() => subjects[0]?.rows ?? [], [subjects]);
  const center = centerMarker(rows, frequencyHz);
  const fits = useMemo(() => bandFits(rows, bands), [rows, bands]);
  const key = `${rows[0]?.frequencyHz}:${rows[rows.length - 1]?.frequencyHz}`;
  const zoom = zoomed?.key === key ? zoomed.domain : null;

  return (
    <div className="flex min-w-0 flex-col gap-4">
      <div className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <Segmented label="Chart" value={chart} options={CHARTS} onChange={setChart} />
          <div className="flex items-center gap-3">
            {zoom !== null && (
              <span className="flex items-center gap-2 font-mono text-[11px] text-ink-dim">
                {formatHz(zoom.low)} to {formatHz(zoom.high)}
                <Button type="button" className={BTN} onClick={() => setZoomed(null)}>
                  Reset zoom
                </Button>
              </span>
            )}
            <Legend subjects={chart === "impedance" ? [] : subjects} />
          </div>
        </div>
        <div className="grid h-[clamp(15rem,38vh,28rem)] min-h-0 grid-cols-1 gap-3 xl:grid-cols-[minmax(0,1fr)_minmax(14rem,0.42fr)]">
          <FrequencyChart
            viewId={chart}
            subjects={shown}
            zoom={zoom}
            bands={bands}
            markers={
              center === null
                ? []
                : [{ id: "f", label: "f", x: center.row.frequencyHz, active: true }]
            }
            onZoom={(domain) => setZoomed({ key, domain })}
            onPick={() => undefined}
            onStep={() => undefined}
          />
          <div className="hidden min-h-0 xl:block">
            <SmithChart
              subjects={subjects}
              markers={center === null ? [] : [center]}
              active={center}
              onPick={() => undefined}
            />
          </div>
        </div>
      </div>

      <div className="grid gap-4 md:grid-cols-2">
        <PatternChart
          title={`Azimuth at ${simulation.takeoff_deg.toFixed(0)}° elevation`}
          kind="azimuth"
          cut={simulation.azimuth}
          peakDbi={simulation.gain_dbi}
          half={false}
        />
        <PatternChart
          title={`Elevation towards ${Math.round(simulation.bearing_deg) % 360}°`}
          kind="elevation"
          cut={simulation.elevation}
          peakDbi={simulation.gain_dbi}
          half={grounded}
        />
      </div>

      <div className="grid gap-x-6 gap-y-4 md:grid-cols-2">
        <MatchingTable
          simulation={simulation}
          frequencyHz={frequencyHz}
          list={matchingList}
          selected={matching?.id ?? "direct"}
          onSelect={onMatching}
        />
        <BandTable
          fits={fits}
          onFocus={(fit) => setZoomed({ key, domain: bandWindow(fit.band) })}
        />
      </div>
    </div>
  );
}

function bandWindow(band: HamBand): Domain {
  const margin = Math.max((band.stopHz - band.startHz) * 0.25, band.startHz * 0.005);
  return { low: band.startHz - margin, high: band.stopHz + margin };
}

function MatchingTable({
  simulation,
  frequencyHz,
  list,
  selected,
  onSelect,
}: {
  simulation: AntennaSimulation;
  frequencyHz: number;
  list: readonly Matching[];
  selected: string;
  onSelect: (id: string) => void;
}) {
  return (
    <section className="flex min-w-0 flex-col gap-1">
      <h4 className={LABEL} title="How the coax could meet the feedpoint. Pick one to see its SWR.">
        Matching
      </h4>
      <table className="w-full font-mono text-xs tabular-nums">
        <thead>
          <tr className="text-left text-[10.5px] text-ink-faint">
            <th className="py-1 font-normal">Network</th>
            <th className="py-1 text-right font-normal">SWR</th>
            <th className="py-1 text-right font-normal">Under 2:1</th>
          </tr>
        </thead>
        <tbody>
          {list.map((matching) => {
            const swr = swrOf(matching.apply(simulation.impedance, frequencyHz));
            const span = swrSpan(simulation.sweep, matching.apply, frequencyHz);
            const quality = matchQuality(swr);
            return (
              <tr
                key={matching.id}
                aria-selected={matching.id === selected}
                title={matching.detail}
                onClick={() => onSelect(matching.id)}
                className={`cursor-pointer border-t border-line/60 ${
                  matching.id === selected
                    ? "bg-accent/10 text-ink"
                    : "text-ink-dim hover:bg-panel-2"
                }`}
              >
                <td className="py-1 pl-1">
                  <span className="font-sans">{matching.label}</span>
                  <span className="ml-2 text-[10.5px] text-ink-faint">{matching.detail}</span>
                </td>
                <td className={`py-1 text-right ${quality.tone}`}>
                  <span className="inline-flex items-center gap-1">
                    <Icon glyph={quality.glyph} size={12} />
                    {formatSwr(swr)}
                  </span>
                </td>
                <td className="py-1 pr-1 text-right">
                  {span === null
                    ? "-"
                    : `${span.openLow || span.openHigh ? "> " : ""}${formatHz(roundedHz(span.highHz - span.lowHz))}`}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </section>
  );
}
