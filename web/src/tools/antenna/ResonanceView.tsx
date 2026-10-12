import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { ALERT, LABEL } from "../../components/controls";
import { formatHz } from "../../components/format";
import { Segmented } from "../../components/Segmented";
import { toolRunQuery } from "../../lib/api";
import type { AntennaRequest } from "../../lib/types";
import { bandFits, type HamBand } from "../nanovna/bands";
import { FrequencyChart, Legend } from "../nanovna/MeasureView";
import type { Domain } from "../nanovna/Plot";
import { BandTable } from "../nanovna/Readouts";
import { antennaReport, antennaRequest } from "./antenna";
import { sweepSubjects } from "./PerformanceView";
import {
  type Matching,
  type Resonance,
  resonances,
  roundedHz,
  simulationOf,
  unavailableReason,
  wideWindow,
} from "./performance";

const WIDE_POINTS = 121;

type ChartId = "swr" | "impedance";

const CHARTS = [
  { value: "swr", label: "SWR" },
  { value: "impedance", label: "Impedance" },
] as const;

const KIND_MARK = { series: "▼", parallel: "▽" } as const;

export function ResonanceView({
  request,
  bands,
  matching,
}: {
  request: AntennaRequest;
  bands: readonly HamBand[];
  matching: Matching | null;
}) {
  const [chart, setChart] = useState<ChartId>("swr");
  const [zoomed, setZoomed] = useState<{ key: string; domain: Domain } | null>(null);
  const wide = wideWindow(request.frequency_hz, WIDE_POINTS);
  const run = useQuery(
    toolRunQuery(
      antennaRequest({
        ...request,
        trim: false,
        sweep: { start_hz: wide.startHz, stop_hz: wide.stopHz, points: wide.points },
      }),
    ),
  );
  const report = antennaReport(run.data);
  const simulation = simulationOf(report);
  const subjects = simulation === null ? [] : sweepSubjects(simulation, matching);
  const found = simulation === null ? [] : resonances(simulation.sweep);
  const rows = subjects[0]?.rows ?? [];
  const fits = bandFits(rows, bands);
  const key = `${rows[0]?.frequencyHz}:${rows[rows.length - 1]?.frequencyHz}`;
  const zoom = zoomed?.key === key ? zoomed.domain : null;
  const reason = unavailableReason(report);

  if (run.isError) {
    return <p className={ALERT}>{run.error.message}</p>;
  }
  if (reason !== null) {
    return <p className="text-xs text-warn">{reason}</p>;
  }
  if (simulation === null) {
    return (
      <div className="flex min-h-64 items-center justify-center rounded-control border border-dashed border-line text-xs text-ink-faint">
        Sweeping {formatHz(wide.startHz)} to {formatHz(wide.stopHz)}…
      </div>
    );
  }

  return (
    <div className={`flex min-w-0 flex-col gap-4 ${run.isFetching ? "opacity-60" : ""}`}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <Segmented label="Chart" value={chart} options={CHARTS} onChange={setChart} />
        <Legend subjects={chart === "impedance" ? [] : subjects} />
      </div>
      <div className="h-[clamp(15rem,40vh,30rem)] min-h-0">
        <FrequencyChart
          viewId={chart}
          subjects={chart === "impedance" ? subjects.slice(-1) : subjects}
          zoom={zoom}
          bands={bands}
          markers={found.map((resonance, index) => ({
            id: `r${index}`,
            label: KIND_MARK[resonance.kind],
            x: resonance.frequencyHz,
            active: false,
          }))}
          onZoom={(domain) => setZoomed({ key, domain })}
          onPick={() => undefined}
          onStep={() => undefined}
        />
      </div>
      <div className="grid gap-x-6 gap-y-4 md:grid-cols-2">
        <ResonanceTable resonances={found} bands={bands} />
        <BandTable
          fits={fits}
          onFocus={(fit) =>
            setZoomed({
              key,
              domain: { low: fit.band.startHz * 0.97, high: fit.band.stopHz * 1.03 },
            })
          }
        />
      </div>
    </div>
  );
}

function ResonanceTable({
  resonances: list,
  bands,
}: {
  resonances: readonly Resonance[];
  bands: readonly HamBand[];
}) {
  return (
    <section className="flex min-w-0 flex-col gap-1">
      <h4
        className={LABEL}
        title="Where the reactance crosses zero. ▼ series: low impedance. ▽ parallel: high impedance."
      >
        Resonances
      </h4>
      {list.length === 0 ? (
        <p className="py-2 text-xs text-ink-faint">No resonance in this range.</p>
      ) : (
        <table className="w-full font-mono text-xs tabular-nums">
          <thead>
            <tr className="text-left text-2xs text-ink-faint">
              <th className="w-6 py-1 font-normal" />
              <th className="py-1 font-normal">Frequency</th>
              <th className="py-1 text-right font-normal">R</th>
              <th className="py-1 text-right font-normal">Band</th>
            </tr>
          </thead>
          <tbody>
            {list.map((resonance) => {
              const band = bands.find(
                (entry) =>
                  entry.startHz <= resonance.frequencyHz && entry.stopHz >= resonance.frequencyHz,
              );
              return (
                <tr
                  key={`${resonance.kind}${resonance.frequencyHz}`}
                  className={`border-t border-line/60 ${band === undefined ? "text-ink-dim" : "text-ink"}`}
                >
                  <td
                    className="py-1 pl-1"
                    title={resonance.kind === "series" ? "Series" : "Parallel"}
                  >
                    {KIND_MARK[resonance.kind]}
                  </td>
                  <td className="py-1">{formatHz(roundedHz(resonance.frequencyHz))}</td>
                  <td className="py-1 text-right">{Math.round(resonance.resistanceOhms)} Ω</td>
                  <td className="py-1 text-right">{band?.label ?? ""}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </section>
  );
}
