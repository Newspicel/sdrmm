import { HzChip, limitOf, type Mode, NumberChip, OptionalNumberChip } from "./kit";

export function SkimmerChips({ params, limits, onParams }: Mode<"cw_skimmer">) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "cw_skimmer", settings: next });
  return (
    <>
      <HzChip
        label="Passband"
        title="CW skimmer passband"
        value={settings.bandwidth_hz}
        limit={limitOf(limits, "bandwidth_hz")}
        onCommit={(bandwidth_hz) => set({ ...settings, bandwidth_hz })}
      />
      <NumberChip
        label="Acquire"
        title="Carrier threshold above the noise floor"
        value={settings.threshold_db}
        {...limitOf(limits, "threshold_db")}
        unit="dB SNR"
        onCommit={(threshold_db) => set({ ...settings, threshold_db })}
      />
      <NumberChip
        label="Signals"
        title="Maximum simultaneous CW signals"
        value={settings.max_signals}
        {...limitOf(limits, "max_signals")}
        onCommit={(max_signals) => set({ ...settings, max_signals })}
      />
      <OptionalNumberChip
        label="Speed"
        title="Morse speed, empty to track each signal"
        placeholder="auto"
        value={settings.wpm ?? null}
        {...limitOf(limits, "wpm")}
        unit="WPM"
        onCommit={(wpm) => set({ ...settings, wpm })}
      />
    </>
  );
}

import { EMPTY, formatHz, PANE, type PanelProps, TABLE_CELL, TABLE_HEAD } from "./panelKit";

export function CwSkimmerView({ records }: PanelProps<"cw_skimmer">) {
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

import { type DecodedRecordOf } from "../../lib/types";

export const CW_SPOT_GROUP_HZ = 50;

export const CW_SPOT_TEXT_LIMIT = 2_000;

export interface CwSignalRow {
  frequencyHz: number;
  offsetHz: number;
  wpm: number;
  snrDb: number;
  text: string;
}

export function cwSignalRows(
  records: readonly DecodedRecordOf<"cw_skimmer">[],
  limit = CW_SPOT_TEXT_LIMIT,
): CwSignalRow[] {
  const signals = new Map<number, CwSignalRow>();
  for (const record of records.toReversed()) {
    const spot = record.event.data;
    const key = Math.round(spot.offset_hz / CW_SPOT_GROUP_HZ);
    const current = signals.get(key);
    signals.set(key, {
      frequencyHz: record.freq_hz + spot.offset_hz,
      offsetHz: spot.offset_hz,
      wpm: spot.wpm,
      snrDb: spot.snr_db,
      text: `${current?.text ?? ""}${spot.text}`.slice(-limit),
    });
  }
  return [...signals.values()].toSorted((left, right) => left.offsetHz - right.offsetHz);
}
