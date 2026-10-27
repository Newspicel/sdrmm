import { BandwidthChip, limitOf, type Mode, NumberChip } from "./kit";

export function IdentChips({ params, limits, onParams }: Mode<"ident">) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "ident", settings: next });
  return (
    <>
      <BandwidthChip
        label="Width"
        title="Search bandwidth"
        valueHz={settings.bandwidth_hz}
        optionsHz={[12_500, 50_000, 100_000, 192_000]}
        onCommit={(bandwidth_hz) => set({ ...settings, bandwidth_hz })}
      />
      <NumberChip
        label="Every"
        title="Milliseconds of signal each report is measured from"
        value={settings.interval_ms}
        {...limitOf(limits, "interval_ms")}
        unit="ms"
        onCommit={(interval_ms) => set({ ...settings, interval_ms })}
      />
      <NumberChip
        label="Detect"
        title="Decibels above the noise floor a signal must reach"
        value={settings.threshold_db}
        {...limitOf(limits, "threshold_db")}
        unit="dB"
        onCommit={(threshold_db) => set({ ...settings, threshold_db })}
      />
    </>
  );
}

import { EMPTY, formatClock, PANE, type PanelProps, Readout, Readouts } from "./panelKit";

export function IdentView({ records }: PanelProps<"ident">) {
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

import { type IdentReport, type IdentSignal, type Modulation } from "../../lib/types";
import { formatBaud, formatHz } from "../format";

export const MODULATION_LABELS: Record<Modulation, string> = {
  none: "no signal",
  carrier: "unmodulated carrier",
  ook: "OOK",
  am: "AM",
  ssb: "SSB",
  fm: "FM",
  fsk2: "2-FSK",
  fsk4: "4-FSK",
  fsk8: "8-FSK",
  psk2: "BPSK",
  psk4: "QPSK",
  ofdm: "OFDM",
  chirp: "chirp",
  noise_like: "noise-like",
  unknown: "unknown",
};

export function modulationLabel(signal: Pick<IdentSignal, "modulation" | "sideband">): string {
  const base = MODULATION_LABELS[signal.modulation] ?? signal.modulation;
  return signal.sideband == null ? base : `${base} (${signal.sideband.toUpperCase()})`;
}

export type IdentField = readonly [label: string, value: string];

export function signalFrequency(signal: Pick<IdentSignal, "frequency_hz">): string {
  return formatHz(signal.frequency_hz);
}

export function identOverview(report: IdentReport): IdentField[] {
  if ((report.signals ?? []).length > 0) {
    return [];
  }
  return [["Loudest bin", `${report.snr_db.toFixed(1)} dB over the noise floor`]];
}

export function identMeasurements(signal: IdentSignal): IdentField[] {
  const fields: IdentField[] = [
    ["Bandwidth", formatHz(signal.bandwidth_hz)],
    ["Off tune", `${Math.round(signal.center_offset_hz)} Hz`],
    ["SNR", `${signal.snr_db.toFixed(1)} dB`],
  ];
  if (signal.symbol_rate_hz != null) {
    fields.push(["Symbol rate", formatBaud(signal.symbol_rate_hz)]);
  }
  if (signal.deviation_hz != null) {
    fields.push(["Deviation", `±${Math.round(signal.deviation_hz)} Hz`]);
  }
  if (signal.burst_ms != null) {
    const period =
      signal.burst_period_ms == null ? "" : ` every ${signal.burst_period_ms.toFixed(1)} ms`;
    fields.push(["Bursts", `${signal.burst_ms.toFixed(2)} ms${period}`]);
  }
  if (signal.ofdm_symbol_us != null) {
    const guard =
      signal.ofdm_guard_us == null ? "" : `, guard ${Math.round(signal.ofdm_guard_us)} µs`;
    fields.push(["OFDM symbol", `${Math.round(signal.ofdm_symbol_us)} µs${guard}`]);
  }
  if (signal.features.duty < 0.99) {
    fields.push(["Duty", `${Math.round(signal.features.duty * 100)}%`]);
  }
  return fields;
}

export function candidateScore(match: { score: number; confirmed?: boolean }): string {
  return match.confirmed === true ? "confirmed" : `${Math.round(match.score * 100)}%`;
}
