import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Download, Save, Target } from "lucide-react";
import { useMemo, useState } from "react";
import { Button } from "../../components/BaseControls";
import { ALERT, BTN, LABEL, TABLE_CELL, TABLE_HEAD } from "../../components/controls";
import { formatHz } from "../../components/format";
import { Icon } from "../../components/Icon";
import { NumberField } from "../../components/NumberField";
import { Popover } from "../../components/Popover";
import { Segmented } from "../../components/Segmented";
import { Select } from "../../components/Select";
import {
  bandPlanQuery,
  bandRegionsQuery,
  createNanoVnaAntenna,
  NANOVNA_LIBRARY_KEY,
  saveNanoVnaMeasurement,
  toolRunQuery,
} from "../../lib/api";
import type {
  AntennaDesign,
  AntennaPart,
  AntennaReport,
  AntennaRequest,
  AntennaSimulation,
} from "../../lib/types";
import { type HamBand, hamBands } from "../nanovna/bands";
import { downloadText, exportFilename, savedSource, touchstoneS1p } from "../nanovna/export";
import { AntennaView } from "./AntennaView";
import {
  antennaReport,
  antennaRequest,
  DEFAULT_MODEL,
  DESIGN_OPTIONS,
  defaultDesign,
  designLabel,
  formatLength,
  GROUND_OPTIONS,
  type LengthUnit,
  MATERIAL_OPTIONS,
  type ModelSettings,
  UNIT_OPTIONS,
  usesFeedline,
} from "./antenna";
import { necDeck, necFilename } from "./nec";
import { PerformanceView } from "./PerformanceView";
import {
  bestMatching,
  type Matching,
  matchings,
  pointsOf,
  simulationOf,
  sweepWindow,
  swrOf,
  swrSpan,
  unavailableReason,
} from "./performance";
import { ResonanceView } from "./ResonanceView";
import { Summary } from "./Summary";

type Tab = "design" | "performance" | "resonances";

const TABS = [
  { value: "design", label: "Design" },
  { value: "performance", label: "Performance" },
  { value: "resonances", label: "Resonances" },
] as const;

const SWEEP_POINTS = 41;
const AUTO = "auto";

export function AntennaPanel() {
  const [frequencyMhz, setFrequencyMhz] = useState(145.5);
  const [design, setDesign] = useState<AntennaDesign>(defaultDesign("dipole"));
  const [velocityFactor, setVelocityFactor] = useState(0.95);
  const [feedlineFactor, setFeedlineFactor] = useState(0.66);
  const [model, setModel] = useState<ModelSettings>(DEFAULT_MODEL);
  const [unit, setUnit] = useState<LengthUnit>("m");
  const [tab, setTab] = useState<Tab>("design");
  const [chosenMatching, setChosenMatching] = useState(AUTO);
  const bands = useHamBands();

  const frequencyHz = frequencyMhz * 1e6;
  const span = sweepWindow(frequencyHz, bands, SWEEP_POINTS);
  const base: AntennaRequest = {
    frequency_hz: frequencyHz,
    velocity_factor: velocityFactor,
    feedline_velocity_factor: feedlineFactor,
    design,
    model,
    sweep: { start_hz: span.startHz, stop_hz: span.stopHz, points: span.points },
  };
  const run = useQuery(toolRunQuery(antennaRequest(base)));
  const report = antennaReport(run.data);
  const simulation = simulationOf(report);
  const trim = useTrim(base, setVelocityFactor);

  const matchingList = useMemo(
    () =>
      simulation === null || report === null
        ? []
        : matchings(simulation.impedance, report.frequency_hz, report.balanced),
    [simulation, report],
  );
  const matching =
    chosenMatching === AUTO
      ? report === null || simulation === null
        ? null
        : bestMatching(matchingList, simulation.impedance, report.frequency_hz)
      : (matchingList.find((entry) => entry.id === chosenMatching) ?? matchingList[0] ?? null);
  const grounded = model.ground !== "free_space";

  return (
    <div className="flex min-h-full flex-col gap-3">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <DesignBar
          frequencyMhz={frequencyMhz}
          design={design}
          bands={bands}
          onFrequency={setFrequencyMhz}
          onDesign={(next) => {
            setDesign(next);
            setChosenMatching(AUTO);
          }}
        />
        <Segmented label="Antenna section" value={tab} options={TABS} onChange={setTab} />
      </div>
      <div className="flex flex-wrap items-end justify-between gap-3">
        <ModelBar
          model={model}
          design={design}
          velocityFactor={velocityFactor}
          feedlineFactor={feedlineFactor}
          unit={unit}
          trimming={trim.busy}
          onModel={setModel}
          onVelocityFactor={setVelocityFactor}
          onFeedlineFactor={setFeedlineFactor}
          onUnit={setUnit}
          onTrim={trim.start}
        />
        {report !== null && simulation !== null && (
          <Actions report={report} simulation={simulation} model={model} matching={matching} />
        )}
      </div>

      {run.isError && <p className={ALERT}>{run.error.message}</p>}
      {trim.note !== null && <p className="text-xs text-warn">{trim.note}</p>}
      {report !== null && (
        <Overview
          report={report}
          simulation={simulation}
          grounded={grounded}
          matching={matching}
          stale={run.isFetching}
        />
      )}

      {report !== null && (
        <div hidden={tab !== "design"}>
          <DesignView report={report} simulation={simulation} unit={unit} />
        </div>
      )}
      {report !== null && simulation !== null && tab === "performance" && (
        <PerformanceView
          simulation={simulation}
          frequencyHz={report.frequency_hz}
          grounded={grounded}
          bands={bands}
          matchingList={matchingList}
          matching={matching}
          onMatching={setChosenMatching}
        />
      )}
      {report !== null && simulation !== null && tab === "resonances" && (
        <ResonanceView request={base} bands={bands} matching={matching} />
      )}
    </div>
  );
}

function useHamBands(): HamBand[] {
  const regions = useQuery(bandRegionsQuery());
  const plan = useQuery(bandPlanQuery(regions.data?.default_region ?? null));
  return useMemo(() => (plan.data === undefined ? [] : hamBands(plan.data)), [plan.data]);
}

function useTrim(base: AntennaRequest, onFactor: (factor: number) => void) {
  const queryClient = useQueryClient();
  const trim = useMutation({
    mutationFn: async () => {
      const response = await queryClient.fetchQuery(
        toolRunQuery(antennaRequest({ ...base, trim: true })),
      );
      return simulationOf(antennaReport(response))?.resonant_velocity_factor ?? null;
    },
    onSuccess: (factor) => {
      if (factor != null) {
        onFactor(Math.round(factor * 10_000) / 10_000);
      }
    },
  });
  const note = trim.isError
    ? trim.error.message
    : trim.isSuccess && trim.data == null
      ? "No resonance within reach of the element factor."
      : null;
  return { busy: trim.isPending, note, start: () => trim.mutate() };
}

function Overview({
  report,
  simulation,
  grounded,
  matching,
  stale,
}: {
  report: AntennaReport;
  simulation: AntennaSimulation | null;
  grounded: boolean;
  matching: Matching | null;
  stale: boolean;
}) {
  const reason = unavailableReason(report);
  if (simulation === null) {
    return reason === null ? null : (
      <p className="rounded-control border border-warn/40 bg-warn/10 px-3 py-1.5 text-xs text-warn">
        {reason}
      </p>
    );
  }
  const transform = matching?.apply ?? ((z) => z);
  return (
    <div className={stale ? "opacity-60 transition-opacity" : "transition-opacity"}>
      <Summary
        simulation={simulation}
        frequencyHz={report.frequency_hz}
        grounded={grounded}
        matching={matching}
        swr={swrOf(transform(simulation.impedance, report.frequency_hz))}
        span={swrSpan(simulation.sweep, transform, report.frequency_hz)}
      />
    </div>
  );
}

function DesignBar({
  frequencyMhz,
  design,
  bands,
  onFrequency,
  onDesign,
}: {
  frequencyMhz: number;
  design: AntennaDesign;
  bands: readonly HamBand[];
  onFrequency: (mhz: number) => void;
  onDesign: (design: AntennaDesign) => void;
}) {
  const hz = frequencyMhz * 1e6;
  const band = bands.find((entry) => entry.startHz <= hz && entry.stopHz >= hz);
  return (
    <div className="flex flex-wrap items-end gap-x-3 gap-y-2">
      <Field label="Design">
        <Select
          label="Antenna design"
          value={design.type}
          options={DESIGN_OPTIONS}
          onChange={(type) => onDesign(defaultDesign(type))}
          className="w-60"
        />
      </Field>
      <Field label="Band">
        <Select
          label="Amateur band"
          value={band?.label ?? ""}
          options={[
            { value: "", label: "Custom" },
            ...bands.map((entry) => ({ value: entry.label, label: entry.label })),
          ]}
          onChange={(label) => {
            const picked = bands.find((entry) => entry.label === label);
            if (picked !== undefined) {
              onFrequency(Math.round((picked.startHz + picked.stopHz) / 2_000) / 1_000);
            }
          }}
          className="w-28"
        />
      </Field>
      <Field label="Frequency">
        <NumberField
          label="Frequency"
          unit="MHz"
          value={frequencyMhz}
          onCommit={onFrequency}
          min={0.01}
          max={300_000}
          step={0.005}
          className="w-32"
        />
      </Field>
      <DesignSettings design={design} onChange={onDesign} />
    </div>
  );
}

function ModelBar({
  model,
  design,
  velocityFactor,
  feedlineFactor,
  unit,
  trimming,
  onModel,
  onVelocityFactor,
  onFeedlineFactor,
  onUnit,
  onTrim,
}: {
  model: ModelSettings;
  design: AntennaDesign;
  velocityFactor: number;
  feedlineFactor: number;
  unit: LengthUnit;
  trimming: boolean;
  onModel: (model: ModelSettings) => void;
  onVelocityFactor: (factor: number) => void;
  onFeedlineFactor: (factor: number) => void;
  onUnit: (unit: LengthUnit) => void;
  onTrim: () => void;
}) {
  const trimmable = design.type !== "quad_loop" && design.type !== "five_eighths_vertical";
  return (
    <div className="flex flex-wrap items-end gap-x-3 gap-y-2">
      <Field label="Element factor">
        <div className="flex items-center gap-1">
          <NumberField
            label="Element velocity factor"
            value={velocityFactor}
            onCommit={onVelocityFactor}
            min={0.5}
            max={1}
            step={0.001}
            className="w-24"
          />
          {trimmable && (
            <Button
              type="button"
              className={BTN}
              disabled={trimming}
              title="Find the element factor where the feedpoint is resonant"
              onClick={onTrim}
            >
              <Icon glyph={Target} size={12} />
              {trimming ? "Trimming…" : "Trim"}
            </Button>
          )}
        </div>
      </Field>
      {usesFeedline(design) && (
        <Field label="Coax factor">
          <NumberField
            label="Feedline velocity factor"
            value={feedlineFactor}
            onCommit={onFeedlineFactor}
            min={0.4}
            max={1}
            step={0.01}
            className="w-20"
          />
        </Field>
      )}
      <Field label="Wire">
        <NumberField
          label="Wire diameter"
          unit="mm"
          value={model.wire_diameter_mm}
          onCommit={(wire_diameter_mm) => onModel({ ...model, wire_diameter_mm })}
          min={0.1}
          max={100}
          step={0.1}
          className="w-24"
        />
      </Field>
      <Field label="Metal">
        <Select
          label="Conductor"
          value={model.material}
          options={MATERIAL_OPTIONS}
          onChange={(material) => onModel({ ...model, material })}
          className="w-32"
        />
      </Field>
      <Field label="Ground">
        <Select
          label="Ground"
          value={model.ground}
          options={GROUND_OPTIONS}
          onChange={(ground) => onModel({ ...model, ground })}
          className="w-32"
        />
      </Field>
      {model.ground !== "free_space" && (
        <Field label="Feed height">
          <NumberField
            label="Feedpoint height above ground"
            unit="m"
            value={model.height_m}
            onCommit={(height_m) => onModel({ ...model, height_m })}
            min={0}
            max={1000}
            step={0.5}
            className="w-24"
          />
        </Field>
      )}
      <Field label="Units">
        <Segmented label="Length units" value={unit} options={UNIT_OPTIONS} onChange={onUnit} />
      </Field>
    </div>
  );
}

function Actions({
  report,
  simulation,
  model,
  matching,
}: {
  report: AntennaReport;
  simulation: AntennaSimulation;
  model: ModelSettings;
  matching: Matching | null;
}) {
  const queryClient = useQueryClient();
  const name = `${designLabel(report.design.type)}, ${formatHz(report.frequency_hz)}`;
  const label =
    matching === null || matching.id === "direct" ? "Simulated" : `Simulated, ${matching.label}`;
  const points = pointsOf(simulation.sweep, matching?.apply);
  const save = useMutation({
    mutationFn: async () => {
      const antenna = await createNanoVnaAntenna({ name, notes: necSummary(model) });
      return saveNanoVnaMeasurement(antenna.id, {
        label,
        note: "",
        instrument: "sdrmm simulation",
        points,
      });
    },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: NANOVNA_LIBRARY_KEY }),
  });
  return (
    <div className="flex items-center gap-1.5">
      {save.isSuccess && <span className="text-xs text-ok">Saved to the NanoVNA library</span>}
      {save.isError && <span className="text-xs text-danger">{save.error.message}</span>}
      <Button
        type="button"
        className={BTN}
        disabled={save.isPending}
        title="Keep this simulated SWR curve next to your NanoVNA measurements"
        onClick={() => save.mutate()}
      >
        <Icon glyph={Save} size={12} />
        Save to NanoVNA
      </Button>
      <Popover
        label={
          <>
            <Icon glyph={Download} size={12} />
            Export
          </>
        }
        triggerClass={BTN}
        width="w-56"
        align="end"
      >
        {(close) => (
          <div className="flex flex-col gap-2">
            <Button
              type="button"
              className={`${BTN} justify-start`}
              onClick={() => {
                downloadText(necFilename(report), "text/plain", necDeck(report, simulation, model));
                close();
              }}
            >
              NEC model .nec
            </Button>
            <Button
              type="button"
              className={`${BTN} justify-start`}
              onClick={() => {
                const source = savedSource(name, points, "sdrmm simulation");
                downloadText(
                  exportFilename(source, "s1p"),
                  "application/octet-stream",
                  touchstoneS1p(source, "ri", { recordedAt: new Date().toISOString() }),
                );
                close();
              }}
            >
              Touchstone .s1p
            </Button>
          </div>
        )}
      </Popover>
    </div>
  );
}

function necSummary(model: ModelSettings): string {
  const ground =
    model.ground === "free_space"
      ? "free space"
      : `${model.height_m} m over ${model.ground} ground`;
  return `Simulated: ${model.wire_diameter_mm} mm ${model.material.replace("_", " ")} wire, ${ground}`;
}

function DesignView({
  report,
  simulation,
  unit,
}: {
  report: AntennaReport;
  simulation: AntennaSimulation | null;
  unit: LengthUnit;
}) {
  const [highlight, setHighlight] = useState<string | null>(null);
  const placed = report.parts.some((part) => part.position_m != null);
  return (
    <div className="grid gap-x-6 gap-y-4 xl:grid-cols-[minmax(0,1.15fr)_minmax(0,1fr)]">
      <AntennaView
        report={report}
        wires={simulation?.wires ?? null}
        unit={unit}
        highlight={highlight}
        onHighlight={setHighlight}
      />
      <div className="flex min-w-0 flex-col gap-3">
        <table className="w-full border-collapse">
          <thead>
            <tr className="border-b border-line">
              <th className={TABLE_HEAD}>Part</th>
              <th className={TABLE_HEAD}>Qty</th>
              <th className={`${TABLE_HEAD} whitespace-nowrap`}>Length</th>
              {placed && <th className={`${TABLE_HEAD} whitespace-nowrap`}>On the boom</th>}
            </tr>
          </thead>
          <tbody>
            {report.parts.map((part) => (
              <Row
                key={part.name}
                part={part}
                unit={unit}
                placed={placed}
                lit={highlight === part.name}
                onHighlight={setHighlight}
              />
            ))}
          </tbody>
        </table>
        {simulation?.loading_coil_h != null && (
          <p className="font-mono text-xs text-ink-dim">
            <span className="text-ink-faint">Base coil </span>
            {(simulation.loading_coil_h * 1e6).toFixed(2)} µH
          </p>
        )}
        <ul className="flex flex-col gap-1.5">
          {report.notes.map((note) => (
            <li key={note} className="flex gap-2 text-xs text-ink-dim">
              <span aria-hidden className="text-ink-faint">
                ·
              </span>
              {note}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

function Row({
  part,
  unit,
  placed,
  lit,
  onHighlight,
}: {
  part: AntennaPart;
  unit: LengthUnit;
  placed: boolean;
  lit: boolean;
  onHighlight: (label: string | null) => void;
}) {
  return (
    <tr
      className={`border-b border-line/60 align-top ${lit ? "bg-panel-2" : ""}`}
      onPointerEnter={() => onHighlight(part.name)}
      onPointerLeave={() => onHighlight(null)}
    >
      <td className={`${TABLE_CELL} text-ink`}>
        {part.name}
        {part.detail != null && <p className="mt-0.5 font-sans text-ink-dim">{part.detail}</p>}
      </td>
      <td className={`${TABLE_CELL} text-ink-dim`}>{part.count > 1 ? `× ${part.count}` : ""}</td>
      <td className={`${TABLE_CELL} whitespace-nowrap text-accent`}>
        {formatLength(part.length_m, unit)}
      </td>
      {placed && (
        <td className={`${TABLE_CELL} whitespace-nowrap text-ink-dim`}>
          {part.position_m == null ? "" : formatLength(part.position_m, unit)}
        </td>
      )}
    </tr>
  );
}

function DesignSettings({
  design,
  onChange,
}: {
  design: AntennaDesign;
  onChange: (design: AntennaDesign) => void;
}) {
  switch (design.type) {
    case "inverted_v":
      return (
        <Field label="Apex angle">
          <NumberField
            label="Apex angle"
            unit="°"
            value={design.settings.apex_angle_deg ?? 120}
            onCommit={(apex_angle_deg) => onChange({ ...design, settings: { apex_angle_deg } })}
            min={60}
            max={180}
            step={5}
            className="w-24"
          />
        </Field>
      );
    case "ground_plane":
      return (
        <>
          <Field label="Radials">
            <NumberField
              label="Radial count"
              value={design.settings.radials ?? 4}
              onCommit={(radials) =>
                onChange({ ...design, settings: { ...design.settings, radials } })
              }
              min={1}
              max={32}
              step={1}
              className="w-20"
            />
          </Field>
          <Field label="Radial slope">
            <NumberField
              label="Radial slope"
              unit="°"
              value={design.settings.radial_slope_deg ?? 45}
              onCommit={(radial_slope_deg) =>
                onChange({ ...design, settings: { ...design.settings, radial_slope_deg } })
              }
              min={0}
              max={60}
              step={5}
              className="w-24"
            />
          </Field>
        </>
      );
    case "yagi":
      return (
        <>
          <Field label="Directors">
            <NumberField
              label="Director count"
              value={design.settings.directors ?? 2}
              onCommit={(directors) =>
                onChange({ ...design, settings: { ...design.settings, directors } })
              }
              min={0}
              max={20}
              step={1}
              className="w-20"
            />
          </Field>
          <Field label="Spacing">
            <NumberField
              label="Element spacing in wavelengths"
              value={design.settings.spacing_wavelengths ?? 0.2}
              onCommit={(spacing_wavelengths) =>
                onChange({ ...design, settings: { ...design.settings, spacing_wavelengths } })
              }
              min={0.1}
              max={0.4}
              step={0.01}
              unit="λ"
              className="w-24"
            />
          </Field>
        </>
      );
    default:
      return null;
  }
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <span className={LABEL}>{label}</span>
      {children}
    </div>
  );
}
