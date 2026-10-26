import { ChevronDown, ChevronUp } from "lucide-react";
import { useLayoutEffect, useRef, useState } from "react";
import { ISM_KIND_LABELS, percentOf, WIFI_BAND_LABELS } from "../lib/airtime";
import { copyText } from "../lib/copyText";
import { useKindLabel } from "../lib/eventKinds";
import { type GsmCellView, gsmCells, gsmSummary } from "../lib/gsm";
import {
  berText,
  sidDeviation,
  sidTrends,
  signedDb,
  sparkPoints,
  wholeDb,
} from "../lib/measurement";
import type { EntityOf } from "../lib/sinks";
import { toastError } from "../lib/toasts";
import type { DecodedRecordOf, IdentSignal } from "../lib/types";
import { useNow } from "../lib/useNow";
import type { PanelProps, Panels } from "../sinks/panels";
import { Button } from "./BaseControls";
import { BroadcastDataView } from "./BroadcastDataView";
import { BTN, TABLE_CELL, TABLE_HEAD } from "./controls";
import {
  type AprsWeatherStation,
  ageClass,
  aprsWeatherStations,
  type BleDevice,
  bleDevices,
  buildTranscript,
  candidateScore,
  cwSignalRows,
  type DectStation,
  type Drone,
  dectStations,
  formatAge,
  formatAltFreqs,
  formatClock,
  formatPosition,
  identMeasurements,
  identOverview,
  isAtBottom,
  type LoraStation,
  latestRecord,
  latestVorReadings,
  latestWpm,
  loraStations,
  modulationLabel,
  multiVorFix,
  ptyLabel,
  type RadiosondeStation,
  radiosondeLog,
  radiosondeStations,
  rdsPicture,
  rdsQuality,
  remoteIdDrones,
  signalFrequency,
  sortTargets,
  type TargetRow,
  type TargetSort,
  targetRow,
  toneLabel,
} from "./decoderViews";
import { Readout, Readouts } from "./face/Readouts";
import { formatHz } from "./format";
import { Icon } from "./Icon";
import { Unit } from "./Unit";
import {
  celsius,
  climbRate,
  hectopascal,
  metres,
  metresPerSecond,
  millimetres,
  percent,
  SONDE_LABELS,
  wind,
} from "./weatherFormat";

const PANE = "flex flex-col gap-2 p-3";
const EMPTY = "text-sm text-ink-dim";

function RdsView({ records }: PanelProps<"rds">) {
  const rds = rdsPicture(records);

  if (rds === null) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>No RDS yet: tune a WFM channel to a station that carries it.</span>
      </div>
    );
  }

  const quality = rdsQuality(rds);
  const altFreqs = formatAltFreqs(rds.alt_freqs_hz);

  return (
    <div className={PANE}>
      <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1">
        <span className="font-mono text-2xl tracking-wide text-ink">
          {rds.ps?.trim() || "········"}
        </span>
        <span className="font-mono text-sm tabular-nums text-ink-dim">PI {rds.pi ?? "-"}</span>
        <span className="text-sm text-ink-dim">{ptyLabel(rds)}</span>
        <div className="ml-auto flex items-center gap-1">
          <Flag label="TP" on={rds.tp === true} />
          <Flag label="TA" on={rds.ta === true} />
          <Flag label={rds.music === false ? "SP" : "MS"} on={rds.music != null} />
        </div>
      </div>

      <div>
        <div className="legend">RadioText</div>
        <div className="overflow-x-auto whitespace-nowrap rounded-control border border-line bg-panel px-2 py-1.5 font-mono text-sm text-ink">
          {rds.radiotext?.trim() || <span className="text-ink-dim">-</span>}
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
        <div className="flex flex-wrap items-center gap-1">
          <span className="legend">AF</span>
          {altFreqs.length === 0 ? (
            <span className="font-mono text-xs text-ink-dim">-</span>
          ) : (
            <span className="font-mono text-xs tabular-nums text-ink">{altFreqs.join(" · ")}</span>
          )}
        </div>
        <div className="ml-auto flex items-center gap-2 font-mono text-xs tabular-nums text-ink-dim">
          <span className="legend">Quality</span>
          <span className={quality.className}>{quality.label}</span>
          <span>
            {quality.groups} groups · {quality.blockErrors} block errors ·{" "}
            {(quality.errorRate * 100).toFixed(1)}%
          </span>
        </div>
      </div>
    </div>
  );
}

function Flag({ label, on }: { label: string; on: boolean }) {
  return (
    <span
      className={`rounded-control border px-1.5 py-0.5 font-mono text-xs ${
        on ? "border-accent text-accent" : "border-line text-ink-dim opacity-45"
      }`}
    >
      {label}
    </span>
  );
}

const TARGET_COLUMNS = {
  adsb: {
    title: "Aircraft",
    idHeader: "ICAO",
    labelHeader: "Callsign",
    primaryHeader: "Altitude",
    secondaryHeader: "Speed / track",
  },
  ais: {
    title: "Ships",
    idHeader: "MMSI",
    labelHeader: "Name",
    primaryHeader: "Speed",
    secondaryHeader: "Course / destination",
  },
} as const;

function TargetsView({ entities }: { entities: readonly (EntityOf<"adsb"> | EntityOf<"ais">)[] }) {
  const now = useNow(1000);
  const [sort, setSort] = useState<TargetSort>("age");
  const [descending, setDescending] = useState(false);

  const toggle = (key: TargetSort): void => {
    if (key === sort) {
      setDescending(!descending);
    } else {
      setSort(key);
      setDescending(false);
    }
  };

  const rows = entities.map((entity) => targetRow(entity, now));
  const kind = entities[0]?.key.kind === "ais" ? "ais" : "adsb";

  return (
    <div className={PANE}>
      <TargetTable
        {...TARGET_COLUMNS[kind]}
        rows={sortTargets(rows, sort, descending)}
        sort={sort}
        descending={descending}
        onSort={toggle}
      />
    </div>
  );
}

function TargetTable({
  title,
  idHeader,
  labelHeader,
  primaryHeader,
  secondaryHeader,
  rows,
  sort,
  descending,
  onSort,
}: {
  title: string;
  idHeader: string;
  labelHeader: string;
  primaryHeader: string;
  secondaryHeader: string;
  rows: readonly TargetRow[];
  sort: TargetSort;
  descending: boolean;
  onSort: (key: TargetSort) => void;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <div className="flex items-baseline gap-2">
        <span className="legend">{title}</span>
        <span className="font-mono text-2xs tabular-nums text-ink-dim">{rows.length}</span>
      </div>
      {rows.length === 0 ? (
        <span className={EMPTY}>No {title.toLowerCase()} heard.</span>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full min-w-[32rem] border-collapse">
            <thead>
              <tr className="border-b border-line">
                <th className={TABLE_HEAD} scope="col">
                  <SortButton
                    label={idHeader}
                    sorted={sort === "id" ? (descending ? "down" : "up") : null}
                    onClick={() => onSort("id")}
                  />
                </th>
                <th className={TABLE_HEAD} scope="col">
                  {labelHeader}
                </th>
                <th className={TABLE_HEAD} scope="col">
                  {primaryHeader}
                </th>
                <th className={TABLE_HEAD} scope="col">
                  {secondaryHeader}
                </th>
                <th className={TABLE_HEAD} scope="col">
                  Position
                </th>
                <th className={TABLE_HEAD} scope="col">
                  <SortButton
                    label="Age"
                    sorted={sort === "age" ? (descending ? "down" : "up") : null}
                    onClick={() => onSort("age")}
                  />
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={row.id} className={`border-b border-line/40 ${ageClass(row.ageMs)}`}>
                  <td className={`${TABLE_CELL} font-semibold`}>{row.id}</td>
                  <td className={TABLE_CELL}>{row.label}</td>
                  <td className={TABLE_CELL}>{row.primary}</td>
                  <td className={TABLE_CELL}>{row.secondary || "-"}</td>
                  <td className={TABLE_CELL}>{row.position}</td>
                  <td className={`${TABLE_CELL} text-right`}>{formatAge(row.ageMs)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function SortButton({
  label,
  sorted,
  onClick,
}: {
  label: string;
  sorted: "up" | "down" | null;
  onClick: () => void;
}) {
  return (
    <Button
      type="button"
      className="inline-flex items-center gap-1 hover:text-accent"
      onClick={onClick}
    >
      {label}
      {sorted !== null && <Icon glyph={sorted === "down" ? ChevronDown : ChevronUp} size={12} />}
    </Button>
  );
}

function TextView({
  kind,
  records,
}: {
  kind: "rtty" | "morse" | "psk";
  records: PanelProps<"rtty" | "morse" | "psk">["records"];
}) {
  const kindLabel = useKindLabel();
  const text = buildTranscript(records);
  const wpm = kind === "morse" ? latestWpm(records as readonly DecodedRecordOf<"morse">[]) : null;
  const paneRef = useRef<HTMLPreElement>(null);
  const stick = useRef(true);

  useLayoutEffect(() => {
    const el = paneRef.current;
    if (el !== null && stick.current) {
      el.scrollTop = el.scrollHeight;
    }
    // oxlint-disable-next-line react/exhaustive-effect-dependencies -- new text is what scrolls the pane
  }, [text]);

  const copy = (): void => {
    copyText(text).catch(toastError);
  };

  return (
    <div className={PANE}>
      <div className="flex items-center gap-3">
        <span className="legend">{kindLabel(kind)}</span>
        {wpm !== null && (
          <span className="font-mono text-xs tabular-nums text-ink">
            {wpm.toFixed(0)} <span className="text-ink-dim">WPM</span>
          </span>
        )}
        <Button type="button" className={`${BTN} ml-auto`} disabled={text === ""} onClick={copy}>
          Copy all
        </Button>
      </div>

      <pre
        ref={paneRef}
        tabIndex={0}
        aria-label={`${kind} transcript`}
        className="max-h-72 min-h-32 flex-1 overflow-auto whitespace-pre-wrap break-words rounded-control border border-line bg-panel px-2 py-1.5 font-mono text-xs text-ink"
        onScroll={(e) => {
          stick.current = isAtBottom(e.currentTarget);
        }}
      >
        {text}
      </pre>
    </div>
  );
}

function CwSkimmerView({ records }: PanelProps<"cw_skimmer">) {
  const rows = cwSignalRows(records);
  return (
    <div className={PANE}>
      <div className="flex items-baseline gap-2">
        <span className="legend">Signals in passband</span>
        <span className="font-mono text-xs text-ink-dim">{rows.length}</span>
      </div>
      {rows.length === 0 ? (
        <span className={EMPTY}>No CW carriers decoded yet.</span>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full min-w-[34rem] border-collapse">
            <thead>
              <tr className="border-b border-line">
                <th className={TABLE_HEAD}>Frequency</th>
                <th className={TABLE_HEAD}>Offset</th>
                <th className={TABLE_HEAD}>Speed</th>
                <th className={TABLE_HEAD}>SNR</th>
                <th className={TABLE_HEAD}>Text</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={Math.round(row.offsetHz)} className="border-b border-line/40">
                  <td className={`${TABLE_CELL} tabular-nums`}>{formatHz(row.frequencyHz)}</td>
                  <td className={`${TABLE_CELL} tabular-nums`}>
                    {row.offsetHz >= 0 ? "+" : ""}
                    {row.offsetHz.toFixed(0)} Hz
                  </td>
                  <td className={`${TABLE_CELL} tabular-nums`}>{row.wpm.toFixed(0)} WPM</td>
                  <td className={`${TABLE_CELL} tabular-nums`}>{row.snrDb.toFixed(0)} dB</td>
                  <td
                    className={`${TABLE_CELL} max-w-[24rem] whitespace-pre-wrap break-words font-mono text-ink`}
                  >
                    {row.text}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function ToneView({ records }: PanelProps<"tone">) {
  const latest = records[0];

  if (latest === undefined) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>No subaudible tone heard.</span>
      </div>
    );
  }

  const status = latest.event.data;
  const label = toneLabel(status);
  return (
    <div className={PANE}>
      <div className="flex flex-wrap items-baseline gap-2">
        <span className="font-mono text-xs tabular-nums text-ink-dim">
          {formatClock(latest.at)}
        </span>
        <span className="font-mono text-xs tabular-nums text-accent">
          {label === "" ? "no tone" : label}
        </span>
        <span className="legend">{status.open ? "open" : "muted"}</span>
      </div>
    </div>
  );
}

function IdentView({ records }: PanelProps<"ident">) {
  const latest = records[0];

  if (latest === undefined) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>Nothing analysed yet.</span>
      </div>
    );
  }

  const report = latest.event.data;
  const signals = report.signals ?? [];

  return (
    <div className={PANE}>
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <span className="font-mono text-2xl tracking-wide text-ink">
          {signals.length === 0
            ? "no signal"
            : `${signals.length} signal${signals.length === 1 ? "" : "s"}`}
        </span>
        <span className="ml-auto font-mono text-xs tabular-nums text-ink-dim">
          {formatClock(latest.at)}
        </span>
      </div>

      {signals.length === 0 && (
        <Readouts ruled={false} padded={false}>
          {identOverview(report).map(([label, value]) => (
            <Readout key={label} label={label}>
              {value}
            </Readout>
          ))}
        </Readouts>
      )}

      {signals.map((signal) => (
        <IdentSignalView key={`${signal.frequency_hz}:${signal.bandwidth_hz}`} signal={signal} />
      ))}
    </div>
  );
}

function IdentSignalView({ signal }: { signal: IdentSignal }) {
  const candidates = signal.candidates ?? [];
  return (
    <div className="flex flex-col gap-1 border-t border-line pt-2 first:border-t-0 first:pt-0">
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <span className="font-mono text-sm tabular-nums text-ink">{signalFrequency(signal)}</span>
        <span className="font-mono text-lg tracking-wide text-ink">{modulationLabel(signal)}</span>
        <span className="legend">{Math.round(signal.confidence * 100)}% confident</span>
      </div>

      <Readouts ruled={false} padded={false}>
        {identMeasurements(signal).map(([label, value]) => (
          <Readout key={label} label={label}>
            {value}
          </Readout>
        ))}
      </Readouts>

      <div>
        <div className="legend">Protocol</div>
        {candidates.length === 0 ? (
          <span className={EMPTY}>Nothing in the catalog fits these measurements.</span>
        ) : (
          <ul className="flex flex-col gap-1">
            {candidates.map((match) => (
              <li key={match.name} className="flex flex-wrap items-baseline gap-2">
                <span
                  className={`font-mono text-sm ${match.confirmed === true ? "text-accent" : "text-ink"}`}
                >
                  {match.name}
                </span>
                <span className="font-mono text-xs tabular-nums text-ink-dim">
                  {candidateScore(match)}
                </span>
                <span className="text-xs text-ink-dim">{match.why}</span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

const DECT_CIPHER_STATE: Record<string, string> = {
  clear: "no encryption seen",
  requested: "start requested",
  confirmed: "start confirmed",
  active: "encrypted",
  stopped: "encryption stopped",
};

function dectSupport(value: boolean | null, second: boolean | null, name: string): string {
  const base = value === null ? "-" : value ? "yes" : "no";
  return second ? `${base} · ${name}` : base;
}

function DectRow({ station }: { station: DectStation }) {
  return (
    <tr>
      <td className={`${TABLE_CELL} font-mono`}>{station.rfpi ?? "-"}</td>
      <td className={TABLE_CELL}>{station.arc === null ? "-" : station.arc.toUpperCase()}</td>
      <td className={TABLE_CELL}>
        {station.carrier === null
          ? "-"
          : station.carrierHz === null
            ? String(station.carrier)
            : `${station.carrier} · ${formatHz(station.carrierHz)}`}
      </td>
      <td className={TABLE_CELL}>{station.slotPair === null ? "-" : String(station.slotPair)}</td>
      <td className={TABLE_CELL}>{dectSupport(station.authentication, station.dsaa2, "DSAA2")}</td>
      <td className={TABLE_CELL}>{dectSupport(station.ciphering, station.dsc2, "DSC2")}</td>
      <td className={TABLE_CELL}>
        {DECT_CIPHER_STATE[station.cipherState] ?? station.cipherState}
      </td>
      <td className={TABLE_CELL}>{station.handsets === 0 ? "-" : String(station.handsets)}</td>
      <td className={TABLE_CELL}>{station.voice ?? "-"}</td>
      <td className={TABLE_CELL}>{station.levelDbfs.toFixed(1)}</td>
      <td className={TABLE_CELL}>{station.bursts}</td>
    </tr>
  );
}

function DectView({ records }: PanelProps<"dect">) {
  const stations = dectStations(records);
  if (stations.length === 0) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>No DECT base stations heard yet.</span>
      </div>
    );
  }
  return (
    <div className={PANE}>
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-left text-xs">
          <thead>
            <tr>
              <th className={TABLE_HEAD}>RFPI</th>
              <th className={TABLE_HEAD}>Class</th>
              <th className={TABLE_HEAD}>Carrier</th>
              <th className={TABLE_HEAD}>Slot</th>
              <th className={TABLE_HEAD}>Auth</th>
              <th className={TABLE_HEAD}>Cipher</th>
              <th className={TABLE_HEAD}>State</th>
              <th className={TABLE_HEAD}>Handsets</th>
              <th className={TABLE_HEAD}>Voice</th>
              <th className={TABLE_HEAD}>
                <Unit symbol="dBFS" />
              </th>
              <th className={TABLE_HEAD}>Bursts</th>
            </tr>
          </thead>
          <tbody>
            {stations.map((station) => (
              <DectRow key={station.key} station={station} />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

const GSM_LOG_ROWS = 50;

function GsmCellReadouts({ cell }: { cell: GsmCellView }) {
  return (
    <Readouts ruled={false} padded={false} columns={2} label={cell.cgi ?? "Cell"}>
      <Readout label="ARFCN" title={cell.band ?? undefined}>
        {cell.arfcn ?? "-"}
      </Readout>
      <Readout label="BSIC" title="Network and base station colour code">
        {cell.bsic}
      </Readout>
      <Readout label="CCCH">{cell.ccch ?? "-"}</Readout>
      <Readout label="Neighbours" title={cell.neighbours.join(", ")}>
        {cell.neighbours.length}
      </Readout>
      <Readout label="SI" title={cell.systemInformation.join(", ")}>
        {cell.systemInformation.length}
      </Readout>
      <Readout label="Paged">{cell.paged}</Readout>
      <Readout label="Offset">{Math.round(cell.frequencyErrorHz)} Hz</Readout>
      <Readout label="SNR">{cell.snrDb.toFixed(1)} dB</Readout>
    </Readouts>
  );
}

function GsmView({ records }: PanelProps<"gsm">) {
  if (records.length === 0) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>No GSM cell heard yet.</span>
      </div>
    );
  }
  const log = records.slice(0, GSM_LOG_ROWS);
  return (
    <div className={PANE}>
      {gsmCells(records).map((cell) => (
        <GsmCellReadouts key={cell.key} cell={cell} />
      ))}
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-left text-xs">
          <thead>
            <tr className="border-b border-line">
              <th className={TABLE_HEAD}>Time</th>
              <th className={TABLE_HEAD}>FN</th>
              <th className={TABLE_HEAD}>TS</th>
              <th className={TABLE_HEAD}>Message</th>
            </tr>
          </thead>
          <tbody>
            {log.map((record) => {
              const frame = record.event.data;
              return (
                <tr
                  key={`${record.at}:${frame.frame_number}:${frame.timeslot}`}
                  className="border-b border-line/40"
                >
                  <td className={`${TABLE_CELL} tabular-nums`}>{formatClock(record.at)}</td>
                  <td className={`${TABLE_CELL} tabular-nums`}>{frame.frame_number}</td>
                  <td className={`${TABLE_CELL} tabular-nums`}>{frame.timeslot}</td>
                  <td className={`${TABLE_CELL} max-w-96 truncate`} title={frame.raw}>
                    {gsmSummary(frame)}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function LoraRow({ station, now }: { station: LoraStation; now: number }) {
  const ageMs = now - Date.parse(station.at);
  return (
    <tr className={`border-b border-line/40 ${ageClass(ageMs)}`}>
      <td className={`${TABLE_CELL} font-mono`}>{station.key}</td>
      <td className={TABLE_CELL}>{station.name ?? "-"}</td>
      <td className={TABLE_CELL}>{station.protocol}</td>
      <td className={`${TABLE_CELL} max-w-64 truncate`} title={station.message}>
        {station.message}
      </td>
      <td className={`${TABLE_CELL} tabular-nums`}>{station.snrDb.toFixed(1)}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{station.frames}</td>
      <td className={`${TABLE_CELL} text-right`}>{formatAge(ageMs)}</td>
    </tr>
  );
}

function wholeMetres(value: number | null): string {
  return value === null ? "-" : value.toFixed(0);
}

function DroneRow({ drone, now }: { drone: Drone; now: number }) {
  const ageMs = now - Date.parse(drone.at);
  return (
    <tr className={`border-b border-line/40 ${ageClass(ageMs)}`}>
      <td className={`${TABLE_CELL} font-mono`}>{drone.key}</td>
      <td className={TABLE_CELL}>{drone.kind}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{wholeMetres(drone.heightM)}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>
        {drone.speedMps === null ? "-" : drone.speedMps.toFixed(1)}
      </td>
      <td className={`${TABLE_CELL} tabular-nums`}>{wholeMetres(drone.pilotM)}</td>
      <td className={`${TABLE_CELL} max-w-40 truncate`} title={drone.operator ?? undefined}>
        {drone.operator ?? "-"}
      </td>
      <td className={TABLE_CELL}>{drone.links.join(", ")}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{drone.levelDbfs.toFixed(0)}</td>
      <td className={`${TABLE_CELL} text-right`}>{formatAge(ageMs)}</td>
    </tr>
  );
}

function RemoteIdView({ records }: PanelProps<"remote_id">) {
  const now = useNow(1000);
  const drones = remoteIdDrones(records);
  if (drones.length === 0) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>No drones heard yet.</span>
      </div>
    );
  }
  return (
    <div className={PANE}>
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-left text-xs">
          <thead>
            <tr className="border-b border-line">
              <th className={TABLE_HEAD}>Drone</th>
              <th className={TABLE_HEAD}>Type</th>
              <th className={TABLE_HEAD}>
                Height <Unit symbol="m" />
              </th>
              <th className={TABLE_HEAD}>
                Speed <Unit symbol="m/s" />
              </th>
              <th className={TABLE_HEAD} title="Distance from drone to pilot">
                Pilot <Unit symbol="m" />
              </th>
              <th className={TABLE_HEAD}>Operator</th>
              <th className={TABLE_HEAD}>Link</th>
              <th className={TABLE_HEAD}>
                Level <Unit symbol="dBFS" />
              </th>
              <th className={TABLE_HEAD}>Heard</th>
            </tr>
          </thead>
          <tbody>
            {drones.map((drone) => (
              <DroneRow key={drone.key} drone={drone} now={now} />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function BleRow({ device, now }: { device: BleDevice; now: number }) {
  const ageMs = now - Date.parse(device.at);
  return (
    <tr className={`border-b border-line/40 ${ageClass(ageMs)}`}>
      <td className={`${TABLE_CELL} font-mono`}>{device.key}</td>
      <td className={`${TABLE_CELL} max-w-40 truncate`} title={device.name ?? undefined}>
        {device.name ?? "-"}
      </td>
      <td className={TABLE_CELL}>{device.vendor ?? "-"}</td>
      <td className={TABLE_CELL}>{device.beacon ?? "-"}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{device.channel ?? "-"}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{device.levelDbfs.toFixed(0)}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{device.heard}</td>
      <td className={`${TABLE_CELL} text-right`}>{formatAge(ageMs)}</td>
    </tr>
  );
}

function BleView({ records }: PanelProps<"ble">) {
  const now = useNow(1000);
  const devices = bleDevices(records);
  if (devices.length === 0) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>No Bluetooth devices heard yet.</span>
      </div>
    );
  }
  return (
    <div className={PANE}>
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-left text-xs">
          <thead>
            <tr className="border-b border-line">
              <th className={TABLE_HEAD}>Address</th>
              <th className={TABLE_HEAD}>Name</th>
              <th className={TABLE_HEAD}>Vendor</th>
              <th className={TABLE_HEAD}>Beacon</th>
              <th className={TABLE_HEAD}>Ch</th>
              <th className={TABLE_HEAD}>
                Level <Unit symbol="dBFS" />
              </th>
              <th className={TABLE_HEAD} title="Adverts heard, repeats included">
                Adverts
              </th>
              <th className={TABLE_HEAD}>Heard</th>
            </tr>
          </thead>
          <tbody>
            {devices.map((device) => (
              <BleRow key={device.key} device={device} now={now} />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function LoadBar({ share }: { share: number }) {
  return (
    <div className="h-2 w-24 overflow-hidden rounded-control bg-line">
      <div className="h-full bg-accent" style={{ width: `${Math.min(100, share * 100)}%` }} />
    </div>
  );
}

function WifiOccupancyView({ records }: PanelProps<"wifi_occupancy">) {
  const latest = latestRecord(records);
  if (latest === null || latest.event.data.channels.length === 0) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>
          {latest === null ? "First report after one second." : "No Wi-Fi channel in view."}
        </span>
      </div>
    );
  }
  return (
    <div className={PANE}>
      <table className="w-full border-collapse text-left text-xs">
        <thead>
          <tr className="border-b border-line">
            <th className={TABLE_HEAD}>Channel</th>
            <th className={TABLE_HEAD}>Busy</th>
            <th className={TABLE_HEAD} />
            <th className={TABLE_HEAD} title="Mean level while busy">
              Level <Unit symbol="dBFS" />
            </th>
            <th className={TABLE_HEAD}>
              Floor <Unit symbol="dBFS" />
            </th>
          </tr>
        </thead>
        <tbody>
          {latest.event.data.channels.map((channel) => (
            <tr key={`${channel.band}-${channel.number}`} className="border-b border-line/40">
              <td className={TABLE_CELL} title={WIFI_BAND_LABELS[channel.band]}>
                {channel.number}
              </td>
              <td className={`${TABLE_CELL} tabular-nums`}>{percentOf(channel.busy)}</td>
              <td className={TABLE_CELL}>
                <LoadBar share={channel.busy} />
              </td>
              <td className={`${TABLE_CELL} tabular-nums`}>
                {channel.level_dbfs == null ? "-" : channel.level_dbfs.toFixed(0)}
              </td>
              <td className={`${TABLE_CELL} tabular-nums`}>
                {channel.floor_dbfs == null ? "-" : channel.floor_dbfs.toFixed(0)}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function IsmSurveyView({ records }: PanelProps<"ism_survey">) {
  const latest = latestRecord(records);
  if (latest === null) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>First report after one second.</span>
      </div>
    );
  }
  const report = latest.event.data;
  return (
    <div className={PANE}>
      <div className="flex items-center gap-2 text-xs">
        <span className="legend">Busy</span>
        <span className="tabular-nums">{percentOf(report.busy)}</span>
        <LoadBar share={report.busy} />
      </div>
      {report.kinds.length === 0 ? (
        <span className={EMPTY}>Nothing on air.</span>
      ) : (
        <table className="w-full border-collapse text-left text-xs">
          <thead>
            <tr className="border-b border-line">
              <th className={TABLE_HEAD}>Kind</th>
              <th className={TABLE_HEAD}>Bursts</th>
              <th className={TABLE_HEAD}>Airtime</th>
              <th className={TABLE_HEAD}>
                Length <Unit symbol="µs" />
              </th>
              <th className={TABLE_HEAD}>
                Peak <Unit symbol="dBFS" />
              </th>
              <th className={TABLE_HEAD}>
                Where <Unit symbol="MHz" />
              </th>
            </tr>
          </thead>
          <tbody>
            {report.kinds.map((load) => (
              <tr key={load.kind} className="border-b border-line/40">
                <td className={TABLE_CELL}>{ISM_KIND_LABELS[load.kind]}</td>
                <td className={`${TABLE_CELL} tabular-nums`}>{load.bursts}</td>
                <td className={`${TABLE_CELL} tabular-nums`}>{percentOf(load.airtime)}</td>
                <td className={`${TABLE_CELL} tabular-nums`}>{Math.round(load.mean_us)}</td>
                <td className={`${TABLE_CELL} tabular-nums`}>{load.peak_dbfs.toFixed(0)}</td>
                <td className={`${TABLE_CELL} max-w-40 truncate tabular-nums`}>
                  {(load.centres_mhz ?? []).join(", ")}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

function LoraView({ records }: PanelProps<"lora">) {
  const now = useNow(1000);
  const stations = loraStations(records);
  if (stations.length === 0) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>No LoRa nodes heard yet.</span>
      </div>
    );
  }
  return (
    <div className={PANE}>
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-left text-xs">
          <thead>
            <tr className="border-b border-line">
              <th className={TABLE_HEAD}>Node</th>
              <th className={TABLE_HEAD}>Name</th>
              <th className={TABLE_HEAD}>Protocol</th>
              <th className={TABLE_HEAD}>Last</th>
              <th className={TABLE_HEAD}>
                SNR <Unit symbol="dB" />
              </th>
              <th className={TABLE_HEAD}>Frames</th>
              <th className={TABLE_HEAD}>Heard</th>
            </tr>
          </thead>
          <tbody>
            {stations.map((station) => (
              <LoraRow key={station.key} station={station} now={now} />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function RadiosondeRow({
  sonde,
  now,
  selected,
  onSelect,
}: {
  sonde: RadiosondeStation;
  now: number;
  selected: boolean;
  onSelect: () => void;
}) {
  const ageMs = now - Date.parse(sonde.at);
  return (
    <tr className={`border-b border-line/40 ${ageClass(ageMs)}`}>
      <td className={`${TABLE_CELL} font-mono`}>
        <Button
          type="button"
          className="hover:text-accent aria-pressed:text-accent"
          aria-pressed={selected}
          title="Show frames"
          onClick={onSelect}
        >
          {sonde.serial}
        </Button>
      </td>
      <td className={TABLE_CELL}>{SONDE_LABELS[sonde.sonde]}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{metres(sonde.altitudeM) ?? "-"}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>
        {metres(sonde.maxAltitudeM) ?? "-"}
        {sonde.burst && (
          <span className="ml-1 text-accent" title="Altitude falling: balloon burst">
            burst
          </span>
        )}
      </td>
      <td className={`${TABLE_CELL} tabular-nums`}>{climbRate(sonde.climbMs) ?? "-"}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{celsius(sonde.temperatureC) ?? "-"}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{percent(sonde.humidityPct) ?? "-"}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{hectopascal(sonde.pressureHpa) ?? "-"}</td>
      <td className={`${TABLE_CELL} text-right`}>{formatAge(ageMs)}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{sonde.frame ?? "-"}</td>
    </tr>
  );
}

const SONDE_LOG_LIMIT = 50;

function RadiosondeLog({ records }: { records: readonly DecodedRecordOf<"radiosonde">[] }) {
  return (
    <div className="max-h-48 overflow-auto">
      <table className="w-full border-collapse text-left text-xs">
        <thead>
          <tr className="border-b border-line">
            <th className={TABLE_HEAD}>Time</th>
            <th className={TABLE_HEAD}>Frame</th>
            <th className={TABLE_HEAD}>Altitude</th>
            <th className={TABLE_HEAD}>Climb</th>
            <th className={TABLE_HEAD}>Temp</th>
            <th className={TABLE_HEAD}>Position</th>
          </tr>
        </thead>
        <tbody>
          {records.map((record) => {
            const frame = record.event.data;
            return (
              <tr key={`${record.at}:${frame.frame ?? ""}`} className="border-b border-line/40">
                <td className={`${TABLE_CELL} tabular-nums`}>{formatClock(record.at)}</td>
                <td className={`${TABLE_CELL} tabular-nums`}>{frame.frame ?? "-"}</td>
                <td className={`${TABLE_CELL} tabular-nums`}>{metres(frame.altitude_m) ?? "-"}</td>
                <td className={`${TABLE_CELL} tabular-nums`}>{climbRate(frame.climb_ms) ?? "-"}</td>
                <td className={`${TABLE_CELL} tabular-nums`}>
                  {celsius(frame.temperature_c) ?? "-"}
                </td>
                <td className={TABLE_CELL}>{formatPosition(frame.lat, frame.lon)}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function RadiosondeView({ records }: PanelProps<"radiosonde">) {
  const now = useNow(1000);
  const sondes = radiosondeStations(records);
  const [selected, setSelected] = useState<string | null>(null);
  const open = sondes.find((sonde) => sonde.serial === selected) ?? sondes[0];
  if (open === undefined) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>No sonde heard yet.</span>
      </div>
    );
  }
  return (
    <div className={PANE}>
      <div className="overflow-x-auto">
        <table className="w-full min-w-[40rem] border-collapse text-left text-xs">
          <thead>
            <tr className="border-b border-line">
              <th className={TABLE_HEAD}>Serial</th>
              <th className={TABLE_HEAD}>Type</th>
              <th className={TABLE_HEAD}>Altitude</th>
              <th className={TABLE_HEAD} title="Highest altitude reached">
                Max
              </th>
              <th className={TABLE_HEAD}>Climb</th>
              <th className={TABLE_HEAD}>Temp</th>
              <th className={TABLE_HEAD} title="Relative humidity">
                RH
              </th>
              <th className={TABLE_HEAD}>Pressure</th>
              <th className={TABLE_HEAD}>Heard</th>
              <th className={TABLE_HEAD}>Frame</th>
            </tr>
          </thead>
          <tbody>
            {sondes.map((sonde) => (
              <RadiosondeRow
                key={sonde.serial}
                sonde={sonde}
                now={now}
                selected={sonde.serial === open.serial}
                onSelect={() => setSelected(sonde.serial)}
              />
            ))}
          </tbody>
        </table>
      </div>
      <span className="legend">{open.serial}</span>
      <RadiosondeLog records={radiosondeLog(records, open.serial, SONDE_LOG_LIMIT)} />
    </div>
  );
}

function AprsWeatherRow({ station, now }: { station: AprsWeatherStation; now: number }) {
  const weather = station.weather;
  const ageMs = now - Date.parse(station.at);
  return (
    <tr className={`border-b border-line/40 ${ageClass(ageMs)}`}>
      <td className={`${TABLE_CELL} font-mono`}>{station.source}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{celsius(weather.temperature_c) ?? "-"}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>
        {station.minTemperatureC === null
          ? "-"
          : `${celsius(station.minTemperatureC)} / ${celsius(station.maxTemperatureC)}`}
      </td>
      <td className={`${TABLE_CELL} tabular-nums`}>{wind(weather) ?? "-"}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{metresPerSecond(station.maxGustMs) ?? "-"}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{percent(weather.humidity_pct) ?? "-"}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{hectopascal(weather.pressure_hpa) ?? "-"}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{millimetres(weather.rain_1h_mm) ?? "-"}</td>
      <td className={`${TABLE_CELL} text-right`}>{formatAge(ageMs)}</td>
    </tr>
  );
}

function AprsWeatherView({ records }: PanelProps<"aprs">) {
  const now = useNow(1000);
  const stations = aprsWeatherStations(records);
  if (stations.length === 0) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>No weather station heard yet.</span>
      </div>
    );
  }
  return (
    <div className={PANE}>
      <div className="overflow-x-auto">
        <table className="w-full min-w-[40rem] border-collapse text-left text-xs">
          <thead>
            <tr className="border-b border-line">
              <th className={TABLE_HEAD}>Station</th>
              <th className={TABLE_HEAD}>Temp</th>
              <th className={TABLE_HEAD} title="Lowest and highest temperature in the kept reports">
                Min / max
              </th>
              <th className={TABLE_HEAD}>Wind</th>
              <th className={TABLE_HEAD} title="Strongest gust in the kept reports">
                Gust
              </th>
              <th className={TABLE_HEAD} title="Relative humidity">
                RH
              </th>
              <th className={TABLE_HEAD}>Pressure</th>
              <th className={TABLE_HEAD} title="Rain in the last hour">
                Rain
              </th>
              <th className={TABLE_HEAD}>Heard</th>
            </tr>
          </thead>
          <tbody>
            {stations.map((station) => (
              <AprsWeatherRow key={station.source} station={station} now={now} />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function VorView({ records }: PanelProps<"vor">) {
  const readings = latestVorReadings(records);
  const fix = multiVorFix(readings);
  if (readings.length === 0) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>No VOR reports yet.</span>
      </div>
    );
  }
  return (
    <div className={PANE}>
      {fix === null ? (
        <span className={EMPTY}>
          Add coordinates to two non-parallel VOR channels to calculate a position fix.
        </span>
      ) : (
        <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1">
          <span className="font-mono text-xl tabular-nums text-ink">
            {fix.lat.toFixed(5)}, {fix.lon.toFixed(5)}
          </span>
          <span className="text-xs text-ink-dim">
            {fix.stations} stations ·{" "}
            {fix.residualKm < 1
              ? `${Math.round(fix.residualKm * 1000)} m`
              : `${fix.residualKm.toFixed(1)} km`}{" "}
            residual
          </span>
        </div>
      )}
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-left text-xs">
          <thead>
            <tr>
              <th className={TABLE_HEAD}>Station</th>
              <th className={TABLE_HEAD}>Radial</th>
              <th className={TABLE_HEAD}>Confidence</th>
              <th className={TABLE_HEAD}>Signal</th>
            </tr>
          </thead>
          <tbody>
            {readings.map((record) => {
              const reading = record.event.data;
              return (
                <tr key={reading.station ?? record.origin?.node ?? ""}>
                  <td className={TABLE_CELL}>{reading.station ?? "-"}</td>
                  <td className={`${TABLE_CELL} font-mono tabular-nums`}>
                    {reading.radial_deg.toFixed(1)}°
                  </td>
                  <td className={`${TABLE_CELL} font-mono tabular-nums`}>
                    {Math.round(reading.confidence * 100)}%
                  </td>
                  <td className={`${TABLE_CELL} font-mono tabular-nums`}>
                    {reading.signal_db.toFixed(1)} dB
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function BroadcastView({ records }: PanelProps<"broadcast">) {
  const status = records[0]?.event.data;
  return (
    <div className={PANE}>
      <span className="legend">{status?.label ?? "Broadcast service"}</span>
      {status?.dynamic_label && <p className="text-sm text-ink">{status.dynamic_label}</p>}
      {!status && <span className={EMPTY}>Waiting for a broadcast service.</span>}
    </div>
  );
}

function BroadcastDataPanel({ records }: PanelProps<"broadcast_data">) {
  const object = records[0]?.event.data;
  return (
    <div className={PANE}>
      {object ? (
        <BroadcastDataView data={object} />
      ) : (
        <span className={EMPTY}>No data object yet.</span>
      )}
    </div>
  );
}

function fixed(value: number | null | undefined, digits = 2): string {
  return value == null ? "-" : value.toFixed(digits);
}

function NoiseFigureView({ records }: PanelProps<"noise_figure">) {
  const latest = latestRecord(records);
  if (latest === null) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>First report after one second.</span>
      </div>
    );
  }
  const report = latest.event.data;
  return (
    <div className={PANE}>
      <div className="flex flex-wrap items-baseline gap-x-3">
        <span className="font-mono text-2xl tabular-nums text-ink">
          {report.nf_db == null ? "NF -" : `NF ${report.nf_db.toFixed(2)}`}
        </span>
        <Unit symbol="dB" />
        <span className="legend ml-auto">
          {report.source_on == null ? "settling" : report.source_on ? "source on" : "source off"}
        </span>
      </div>
      <Readouts ruled={false} padded={false} columns={2}>
        <Readout label="Y-factor">{fixed(report.y_db)} dB</Readout>
        <Readout label="Te" title="Noise temperature">
          {report.temperature_k == null ? "-" : Math.round(report.temperature_k)} K
        </Readout>
        <Readout label="On">{fixed(report.hot_dbfs)} dBFS</Readout>
        <Readout label="Off">{fixed(report.cold_dbfs)} dBFS</Readout>
        <Readout label="Level">{fixed(report.level_dbfs)} dBFS</Readout>
        <Readout label="Settled">{report.settled_s.toFixed(1)} s</Readout>
      </Readouts>
    </div>
  );
}

function PerView({ records }: PanelProps<"per">) {
  const latest = latestRecord(records);
  if (latest === null) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>Waiting for test frames.</span>
      </div>
    );
  }
  const report = latest.event.data;
  return (
    <div className={PANE}>
      <div className="flex flex-wrap items-baseline gap-x-3">
        <span className="font-mono text-2xl tabular-nums text-ink">
          PER {percentOf(report.per)}
        </span>
        <span className="legend ml-auto tabular-nums">
          {report.received} / {report.expected}
        </span>
      </div>
      <Readouts ruled={false} padded={false} columns={2}>
        <Readout label="BER">{report.ber == null ? "-" : berText(report.ber)}</Readout>
        <Readout label="Missed">{report.missed}</Readout>
        <Readout label="CRC failed" tone={report.errored > 0 ? "warn" : undefined}>
          {report.errored}
        </Readout>
        <Readout label="Last frame">{report.last_sequence ?? "-"}</Readout>
      </Readouts>
    </div>
  );
}

const SPARK_WIDTH = 96;
const SPARK_HEIGHT = 18;

function Spark({ values }: { values: readonly number[] }) {
  return (
    <svg
      width={SPARK_WIDTH}
      height={SPARK_HEIGHT}
      viewBox={`0 0 ${SPARK_WIDTH} ${SPARK_HEIGHT}`}
      className="text-accent"
      aria-hidden="true"
    >
      <polyline
        points={sparkPoints(values, SPARK_WIDTH, SPARK_HEIGHT)}
        fill="none"
        stroke="currentColor"
        strokeWidth="1.25"
      />
    </svg>
  );
}

function SidView({ records }: PanelProps<"sid">) {
  const latest = latestRecord(records);
  if (latest === null) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>First report after one interval.</span>
      </div>
    );
  }
  const report = latest.event.data;
  if (report.stations.length === 0) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>No VLF station in view.</span>
      </div>
    );
  }
  const trends = sidTrends(records.map((record) => ({ at: record.at, report: record.event.data })));
  return (
    <div className={PANE}>
      <table className="w-full border-collapse text-left text-xs">
        <thead>
          <tr className="border-b border-line">
            <th className={TABLE_HEAD}>Station</th>
            <th className={TABLE_HEAD}>
              <Unit symbol="kHz" />
            </th>
            <th className={TABLE_HEAD}>
              SNR <Unit symbol="dB" />
            </th>
            <th className={TABLE_HEAD} title="Level against the learned baseline">
              Δ
            </th>
            <th className={TABLE_HEAD} />
          </tr>
        </thead>
        <tbody>
          {report.stations.map((station) => {
            const deviation = sidDeviation(station);
            return (
              <tr
                key={`${station.call}:${station.frequency_hz}`}
                className={`border-b border-line/40 ${station.disturbed ? "text-warn" : ""}`}
              >
                <td className={TABLE_CELL}>{station.call}</td>
                <td className={`${TABLE_CELL} tabular-nums`}>
                  {(station.frequency_hz / 1000).toFixed(2)}
                </td>
                <td className={`${TABLE_CELL} tabular-nums`}>{wholeDb(station.snr_db)}</td>
                <td className={`${TABLE_CELL} tabular-nums`}>
                  {deviation == null ? "-" : signedDb(deviation)}
                </td>
                <td className={TABLE_CELL}>
                  <Spark values={trends.get(`${station.call}:${station.frequency_hz}`) ?? []} />
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

export const PANELS: Panels = {
  rds: (props) => <RdsView {...props} />,
  adsb: (props) => <TargetsView entities={props.entities} />,
  ais: (props) => <TargetsView entities={props.entities} />,
  rtty: (props) => <TextView kind="rtty" records={props.records} />,
  morse: (props) => <TextView kind="morse" records={props.records} />,
  psk: (props) => <TextView kind="psk" records={props.records} />,
  cw_skimmer: (props) => <CwSkimmerView {...props} />,
  tone: (props) => <ToneView {...props} />,
  ident: (props) => <IdentView {...props} />,
  aprs: (props) => <AprsWeatherView {...props} />,
  broadcast: (props) => <BroadcastView {...props} />,
  broadcast_data: (props) => <BroadcastDataPanel {...props} />,
  radiosonde: (props) => <RadiosondeView {...props} />,
  vor: (props) => <VorView {...props} />,
  dect: (props) => <DectView {...props} />,
  gsm: (props) => <GsmView {...props} />,
  lora: (props) => <LoraView {...props} />,
  remote_id: (props) => <RemoteIdView {...props} />,
  ble: (props) => <BleView {...props} />,
  wifi_occupancy: (props) => <WifiOccupancyView {...props} />,
  ism_survey: (props) => <IsmSurveyView {...props} />,
  noise_figure: (props) => <NoiseFigureView {...props} />,
  per: (props) => <PerView {...props} />,
  sid: (props) => <SidView {...props} />,
};
