import { useMemo, useState } from "react";
import { Button } from "../../components/BaseControls";
import { BTN } from "../../components/controls";
import { formatHz } from "../../components/format";
import { Segmented } from "../../components/Segmented";
import type { NanoVnaPoint } from "../../lib/types";
import { AntennaLibrary, type Recording } from "./AntennaLibrary";
import { analyse, readouts } from "./analysis";
import { frequencyTicks, linearTicks } from "./axis";
import { bandFits, bandsWithin, type HamBand } from "./bands";
import { type ActiveMarker, type Marker, markerRows, placeMarker, stepMarker } from "./markers";
import { type Domain, Plot, type Reading } from "./Plot";
import { AnalyzerReadout, BandTable, MarkerTable, MatchPanel } from "./Readouts";
import { dropReference, type Reference, type ReferenceSource, toggleReference } from "./references";
import { SmithChart } from "./SmithChart";
import { TdrView } from "./TdrView";
import {
  LIVE_COLOR,
  nearestIndex,
  type Subject,
  tracesFor,
  VIEWS,
  type ViewId,
  viewFor,
  visibleValues,
} from "./views";

const VIEW_OPTIONS = [
  ...VIEWS.map((view) => ({ value: view.id, label: view.label, title: view.title })),
  { value: "tdr" as const, label: "Distance", title: "Time domain: reflections along the cable" },
];

export interface Primary {
  key: string;
  label: string;
  points: readonly NanoVnaPoint[];
}

export function MeasureView({
  primary,
  recording,
  bands,
  onOpen,
  onHarmonicGrid,
}: {
  primary: Primary | null;
  recording: Recording | null;
  bands: readonly HamBand[];
  onOpen: (source: ReferenceSource) => void;
  onHarmonicGrid: () => void;
}) {
  const [references, setReferences] = useState<Reference[]>([]);

  return (
    <div className="grid min-h-0 flex-1 grid-cols-1 gap-4 lg:grid-cols-[minmax(0,1fr)_17rem]">
      {primary === null ? (
        <div className="flex min-h-64 items-center justify-center rounded-[3px] border border-dashed border-line text-xs text-ink-faint">
          Sweep, or open a saved measurement.
        </div>
      ) : (
        <Workbench
          primary={primary}
          references={references}
          bands={bands}
          onHarmonicGrid={onHarmonicGrid}
        />
      )}
      <AntennaLibrary
        current={recording}
        references={references}
        onToggle={(source) => setReferences((list) => toggleReference(list, source))}
        onOpen={onOpen}
        onForget={(key) => setReferences((list) => dropReference(list, key))}
      />
    </div>
  );
}

function Workbench({
  primary,
  references,
  bands,
  onHarmonicGrid,
}: {
  primary: Primary;
  references: readonly Reference[];
  bands: readonly HamBand[];
  onHarmonicGrid: () => void;
}) {
  const [viewId, setViewId] = useState<ViewId>("swr");
  const [zoomed, setZoomed] = useState<{ range: string; domain: Domain } | null>(null);
  const [markers, setMarkers] = useState<Marker[]>([]);
  const [active, setActive] = useState<ActiveMarker>("best");
  const rows = useMemo(() => readouts(primary.points), [primary.points]);
  const analysis = useMemo(() => analyse(primary.points), [primary.points]);
  const subjects = useMemo(
    () => subjectsFor(primary, rows, references),
    [primary, rows, references],
  );
  const markerList = markerRows(markers, analysis.resonance, rows);
  const activeRow = markerList.find((marker) => marker.id === active) ?? markerList[0] ?? null;
  const fits = useMemo(() => bandFits(rows, bands), [rows, bands]);
  const range = `${rows[0]?.frequencyHz}:${rows[rows.length - 1]?.frequencyHz}`;
  const zoom = zoomed?.range === range ? zoomed.domain : null;
  const setZoom = (domain: Domain | null) => setZoomed(domain === null ? null : { range, domain });

  function pick(hz: number) {
    const placed = placeMarker(markers, active, hz);
    setMarkers(placed.markers);
    setActive(placed.active);
  }

  return (
    <div className="flex min-h-0 min-w-0 flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <Segmented label="Chart" value={viewId} options={VIEW_OPTIONS} onChange={setViewId} />
        <Legend subjects={viewId === "impedance" || viewId === "tdr" ? [] : subjects} />
      </div>
      <div className="grid h-[clamp(16rem,44vh,34rem)] min-h-0 grid-cols-1 gap-3 xl:grid-cols-[minmax(0,1fr)_minmax(14rem,0.42fr)]">
        {viewId === "tdr" ? (
          <TdrView points={primary.points} onHarmonicGrid={onHarmonicGrid} />
        ) : (
          <FrequencyChart
            viewId={viewId}
            subjects={subjects}
            zoom={zoom}
            bands={bands}
            markers={markerList.map((marker) => ({
              id: String(marker.id),
              label: marker.label,
              x: marker.row.frequencyHz,
              active: marker.id === activeRow?.id,
            }))}
            onZoom={setZoom}
            onPick={pick}
            onStep={(delta) => setMarkers(stepMarker(markers, active, rows, delta))}
          />
        )}
        <div className="hidden min-h-0 xl:block">
          <SmithChart
            subjects={zoom === null || viewId === "tdr" ? subjects : within(subjects, zoom)}
            markers={markerList}
            active={activeRow}
            onPick={pick}
          />
        </div>
      </div>
      {zoom !== null && viewId !== "tdr" && (
        <div className="-mt-1 flex items-center gap-2 font-mono text-[11px] text-ink-dim">
          {formatHz(zoom.low)} to {formatHz(zoom.high)}
          <Button type="button" className={BTN} onClick={() => setZoom(null)}>
            Reset zoom
          </Button>
        </div>
      )}
      {activeRow !== null && <AnalyzerReadout marker={activeRow} />}
      <div className="grid gap-x-6 gap-y-4 md:grid-cols-2 2xl:grid-cols-3">
        <MarkerTable
          markers={markerList}
          active={activeRow?.id ?? "best"}
          onActivate={setActive}
          onRemove={(id) => {
            setMarkers(markers.filter((marker) => marker.id !== id));
            setActive("best");
          }}
        />
        <BandTable fits={fits} onFocus={(fit) => setZoom(bandWindow(fit.band))} />
        {activeRow !== null && <MatchPanel row={activeRow.row} />}
      </div>
    </div>
  );
}

function FrequencyChart({
  viewId,
  subjects,
  zoom,
  bands,
  markers,
  onZoom,
  onPick,
  onStep,
}: {
  viewId: Exclude<ViewId, "tdr">;
  subjects: readonly Subject[];
  zoom: Domain | null;
  bands: readonly HamBand[];
  markers: { id: string; label: string; x: number; active: boolean }[];
  onZoom: (domain: Domain) => void;
  onPick: (hz: number) => void;
  onStep: (delta: number) => void;
}) {
  const view = viewFor(viewId);
  const traces = tracesFor(view, subjects);
  const rows = subjects[0]?.rows ?? [];
  const full = {
    low: rows[0]?.frequencyHz ?? 0,
    high: rows[rows.length - 1]?.frequencyHz ?? 1,
  };
  const xDomain = zoom ?? full;
  const yDomain = view.domain(visibleValues(traces, xDomain));

  function readAt(hz: number): Reading | null {
    const live =
      rows[
        nearestIndex(
          rows.map((row) => row.frequencyHz),
          hz,
        )
      ];
    if (live === undefined) {
      return null;
    }
    return {
      title: formatHz(live.frequencyHz),
      rows: traces.map((trace) => {
        const index = nearestIndex(trace.xs, live.frequencyHz);
        const near =
          Math.abs((trace.xs[index] ?? Number.NaN) - live.frequencyHz) <= spacing(trace.xs);
        const value = trace.values[index];
        return {
          key: trace.key,
          label: trace.label,
          color: trace.color,
          value: near && value !== undefined ? view.format(value) : "-",
        };
      }),
    };
  }

  return (
    <Plot
      label={`${view.title} against frequency`}
      xDomain={xDomain}
      yDomain={yDomain}
      xTicks={(domain, width) => frequencyTicks(domain.low, domain.high, width / 110)}
      yTicks={(domain, height) => linearTicks(domain.low, domain.high, height / 46)}
      traces={traces}
      bands={bandsWithin(bands, xDomain.low, xDomain.high).map((band) => ({
        label: band.label,
        low: band.startHz,
        high: band.stopHz,
      }))}
      guides={view.guides.filter(
        (guide) => guide.value > yDomain.low && guide.value < yDomain.high,
      )}
      markers={markers}
      readAt={readAt}
      onPick={onPick}
      onZoom={onZoom}
      onStep={onStep}
    />
  );
}

function Legend({ subjects }: { subjects: readonly Subject[] }) {
  if (subjects.length < 2) {
    return null;
  }
  return (
    <ul className="flex flex-wrap items-center gap-x-4 gap-y-1">
      {subjects.map((subject) => (
        <li key={subject.key} className="flex items-center gap-1.5 text-xs text-ink-dim">
          <svg width="18" height="6" aria-hidden className="shrink-0">
            <line
              x1="0"
              x2="18"
              y1="3"
              y2="3"
              stroke={subject.color}
              strokeWidth="2"
              strokeDasharray={subject.dashed ? "5 3" : undefined}
            />
          </svg>
          <span className="max-w-48 truncate">{subject.label}</span>
        </li>
      ))}
    </ul>
  );
}

function subjectsFor(
  primary: Primary,
  rows: ReturnType<typeof readouts>,
  references: readonly Reference[],
): Subject[] {
  return [
    { key: primary.key, label: primary.label, color: LIVE_COLOR, dashed: false, rows },
    ...references
      .filter((reference) => reference.key !== primary.key)
      .map((reference) => ({
        key: reference.key,
        label: reference.label,
        color: reference.color,
        dashed: true,
        rows: readouts(reference.points),
      })),
  ];
}

function within(subjects: readonly Subject[], window: Domain): Subject[] {
  return subjects.map((subject) => ({
    ...subject,
    rows: subject.rows.filter(
      (row) => row.frequencyHz >= window.low && row.frequencyHz <= window.high,
    ),
  }));
}

function bandWindow(band: HamBand): Domain {
  const margin = Math.max((band.stopHz - band.startHz) * 0.25, band.startHz * 0.005);
  return { low: band.startHz - margin, high: band.stopHz + margin };
}

function spacing(xs: readonly number[]): number {
  const first = xs[0] ?? 0;
  const last = xs[xs.length - 1] ?? 0;
  return xs.length > 1 ? ((last - first) / (xs.length - 1)) * 1.5 : Number.POSITIVE_INFINITY;
}
