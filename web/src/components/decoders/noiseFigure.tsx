import { limitOf, type Mode, NumberChip } from "./kit";

export function NoiseFigureChips({ params, limits, onParams }: Mode<"noise_figure">) {
  return (
    <NumberChip
      label="ENR"
      title="Excess noise ratio of the noise source at this frequency"
      value={params.settings.enr_db}
      {...limitOf(limits, "enr_db")}
      unit="dB"
      onCommit={(enr_db) => onParams({ type: "noise_figure", settings: { enr_db } })}
    />
  );
}

import { EMPTY, latestRecord, PANE, type PanelProps, Readout, Readouts, Unit } from "./panelKit";

function fixed(value: number | null | undefined, digits = 2): string {
  return value == null ? "-" : value.toFixed(digits);
}

export function NoiseFigureView({ records }: PanelProps<"noise_figure">) {
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
