import { limitOf, type Mode, NumberChip } from "./kit";

export function SidChips({ params, limits, onParams }: Mode<"sid">) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "sid", settings: next });
  return (
    <>
      <NumberChip
        label="Every"
        title="Averaging interval of each report"
        value={settings.report_s}
        {...limitOf(limits, "report_s")}
        unit="s"
        onCommit={(report_s) => set({ ...settings, report_s })}
      />
      <NumberChip
        label="Threshold"
        title="Level change against the baseline that counts as a disturbance"
        value={settings.threshold_db}
        {...limitOf(limits, "threshold_db")}
        unit="dB"
        onCommit={(threshold_db) => set({ ...settings, threshold_db })}
      />
    </>
  );
}

import {
  EMPTY,
  latestRecord,
  PANE,
  type PanelProps,
  sidDeviation,
  sidTrends,
  signedDb,
  sparkPoints,
  TABLE_CELL,
  TABLE_HEAD,
  Unit,
  wholeDb,
} from "./panelKit";

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

export function SidView({ records }: PanelProps<"sid">) {
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
