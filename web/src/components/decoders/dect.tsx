import { ChoiceChip, choicesOf, type Mode } from "./kit";

export function DectChips({ params, choices, onParams }: Mode<"dect">) {
  const settings = params.settings;
  return (
    <>
      <ChoiceChip
        label="Band"
        title="DECT band"
        value={settings.band}
        options={choicesOf(choices, params.settings, "band")}
        onChange={(band) => onParams({ type: "dect", settings: { ...settings, band } })}
      />
      <ChoiceChip
        label="Span"
        title="DECT span"
        value={settings.span}
        options={choicesOf(choices, params.settings, "span")}
        onChange={(span) => onParams({ type: "dect", settings: { ...settings, span } })}
      />
      <ChoiceChip
        label="Side"
        title="DECT side"
        value={settings.sides}
        options={choicesOf(choices, params.settings, "sides")}
        onChange={(sides) => onParams({ type: "dect", settings: { ...settings, sides } })}
      />
    </>
  );
}

import { EMPTY, formatHz, PANE, type PanelProps, TABLE_CELL, TABLE_HEAD, Unit } from "./panelKit";

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

export function DectView({ records }: PanelProps<"dect">) {
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

import { type DecodedRecordOf } from "../../lib/types";

export interface DectStation {
  key: string;
  rfpi: string | null;
  arc: string | null;
  carrier: number | null;
  carrierHz: number | null;
  slotPair: number | null;
  authentication: boolean | null;
  ciphering: boolean | null;
  dsaa2: boolean | null;
  dsc2: boolean | null;
  cipherState: string;
  voice: "playing" | "muted" | null;
  handsets: number;
  bursts: number;
  levelDbfs: number;
  at: string;
}

export function dectStations(records: readonly DecodedRecordOf<"dect">[]): DectStation[] {
  const latest = new Map<string, DectStation>();
  for (const record of records) {
    const frame = record.event.data;
    const key = frame.identity?.rfpi ?? `${record.origin?.node ?? ""}:${frame.side}`;
    const previous = latest.get(key);
    if (previous !== undefined && Date.parse(record.at) <= Date.parse(previous.at)) {
      continue;
    }
    const voice = frame.voice ? (frame.voice.playing ? "playing" : "muted") : null;
    if (previous !== undefined && frame.side === "pp") {
      latest.set(key, { ...previous, voice: voice ?? previous.voice, at: record.at });
      continue;
    }
    latest.set(key, {
      key,
      rfpi: frame.identity?.rfpi ?? null,
      arc: frame.identity?.arc ?? null,
      carrier: frame.carrier ?? null,
      carrierHz: frame.carrier_hz ?? null,
      slotPair: frame.slot_pair ?? null,
      authentication: frame.security.authentication_supported ?? null,
      ciphering: frame.security.ciphering_supported ?? null,
      dsaa2: frame.security.dsaa2_supported ?? null,
      dsc2: frame.security.dsc2_supported ?? null,
      cipherState: frame.security.cipher_state,
      voice: voice ?? previous?.voice ?? null,
      handsets: (frame.handsets ?? []).length,
      bursts: frame.bursts,
      levelDbfs: frame.level_dbfs,
      at: record.at,
    });
  }
  return [...latest.values()].toSorted((a, b) => b.levelDbfs - a.levelDbfs);
}
