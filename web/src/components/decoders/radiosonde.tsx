import {
  ChoiceChip,
  choiceOptions,
  limitOf,
  type Mode,
  NumberChip,
  SONDE_AUTO,
  SONDE_BURST_M,
  type SondeType,
  unsetAs,
} from "./kit";

export function RadiosondeChips({ params, choices, limits, onParams }: Mode<"radiosonde">) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "radiosonde", settings: next });
  const sonde = settings.sonde ?? SONDE_AUTO;
  return (
    <>
      <ChoiceChip
        label="Sonde"
        title="Sonde type"
        value={sonde}
        options={unsetAs(choiceOptions<SondeType | null>(choices, "sonde"), SONDE_AUTO)}
        quiet={sonde === SONDE_AUTO}
        onChange={(next) => set({ ...settings, sonde: next === SONDE_AUTO ? null : next })}
      />
      <NumberChip
        label="Burst"
        title="Burst altitude assumed while a sonde climbs"
        value={settings.burst_m ?? SONDE_BURST_M}
        {...limitOf(limits, "burst_m")}
        unit="m"
        onCommit={(burst_m) => set({ ...settings, burst_m })}
      />
      <NumberChip
        label="Ground"
        title="Ground altitude where sondes land"
        value={settings.ground_m ?? 0}
        {...limitOf(limits, "ground_m")}
        unit="m"
        onCommit={(ground_m) => set({ ...settings, ground_m })}
      />
    </>
  );
}

import { useState } from "react";
import {
  ageClass,
  Button,
  celsius,
  climbRate,
  EMPTY,
  formatAge,
  formatClock,
  formatPosition,
  hectopascal,
  metres,
  PANE,
  type PanelProps,
  percent,
  SONDE_LABELS,
  TABLE_CELL,
  TABLE_HEAD,
  useNow,
} from "./panelKit";

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

export function RadiosondeView({ records }: PanelProps<"radiosonde">) {
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

import { type DecodedRecordOf, type RadiosondeFrame } from "../../lib/types";
import { chronological, highest } from "../decoderViews";

export const BURST_DROP_M = 200;

export interface RadiosondeStation {
  serial: string;
  sonde: RadiosondeFrame["sonde"];
  frame: number | null;
  lat: number | null;
  lon: number | null;
  altitudeM: number | null;
  climbMs: number | null;
  temperatureC: number | null;
  humidityPct: number | null;
  pressureHpa: number | null;
  maxAltitudeM: number | null;
  burst: boolean;
  frames: number;
  at: string;
}

export function radiosondeStations(
  records: readonly DecodedRecordOf<"radiosonde">[],
): RadiosondeStation[] {
  const sondes = new Map<string, RadiosondeStation>();
  for (const record of chronological(records)) {
    const frame = record.event.data;
    sondes.set(frame.serial, nextSonde(sondes.get(frame.serial), frame, record.at));
  }
  return [...sondes.values()].toSorted((a, b) => Date.parse(b.at) - Date.parse(a.at));
}

export function nextSonde(
  previous: RadiosondeStation | undefined,
  frame: RadiosondeFrame,
  at: string,
): RadiosondeStation {
  const altitudeM = frame.altitude_m ?? previous?.altitudeM ?? null;
  const maxAltitudeM = highest(previous?.maxAltitudeM ?? null, altitudeM);
  return {
    serial: frame.serial,
    sonde: frame.sonde,
    frame: frame.frame ?? previous?.frame ?? null,
    lat: frame.lat ?? previous?.lat ?? null,
    lon: frame.lon ?? previous?.lon ?? null,
    altitudeM,
    climbMs: frame.climb_ms ?? previous?.climbMs ?? null,
    temperatureC: frame.temperature_c ?? previous?.temperatureC ?? null,
    humidityPct: frame.humidity_pct ?? previous?.humidityPct ?? null,
    pressureHpa: frame.pressure_hpa ?? previous?.pressureHpa ?? null,
    maxAltitudeM,
    burst:
      (previous?.burst ?? false) ||
      (altitudeM !== null && maxAltitudeM !== null && maxAltitudeM - altitudeM > BURST_DROP_M),
    frames: (previous?.frames ?? 0) + 1,
    at,
  };
}

export function radiosondeLog(
  records: readonly DecodedRecordOf<"radiosonde">[],
  serial: string,
  limit: number,
): DecodedRecordOf<"radiosonde">[] {
  return records.filter((record) => record.event.data.serial === serial).slice(0, limit);
}
