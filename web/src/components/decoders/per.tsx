import { ChoiceChip, choicesOf, limitOf, type Mode, NumberChip } from "./kit";

export function PerChips({ params, choices, limits, onParams }: Mode<"per">) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "per", settings: next });
  return (
    <>
      <ChoiceChip
        label="Baud"
        title="2-FSK symbol rate of the test frames"
        value={settings.baud}
        options={choicesOf(choices, params.settings, "baud")}
        onChange={(baud) => set({ ...settings, baud })}
      />
      <NumberChip
        label="Payload"
        title="PRBS9 payload length of each frame"
        value={settings.payload_bytes}
        {...limitOf(limits, "payload_bytes")}
        unit="B"
        onCommit={(payload_bytes) => set({ ...settings, payload_bytes })}
      />
    </>
  );
}

import {
  berText,
  EMPTY,
  latestRecord,
  PANE,
  type PanelProps,
  percentOf,
  Readout,
  Readouts,
} from "./panelKit";

export function PerView({ records }: PanelProps<"per">) {
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
