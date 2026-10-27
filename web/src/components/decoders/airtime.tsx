import { ChoiceChip, choicesOf, limitOf, type Mode, NumberChip } from "./kit";

export function AirtimeChips({
  params,
  choices,
  limits,
  onParams,
}: Mode<"wifi_occupancy" | "ism_survey">) {
  const { type, settings } = params;
  return (
    <>
      <ChoiceChip
        label="Span"
        title="Width of the window measured, in MHz"
        value={settings.span ?? "mhz20"}
        options={choicesOf(choices, params.settings, "span")}
        onChange={(span) => onParams({ type, settings: { ...settings, span } })}
      />
      <NumberChip
        label="Margin"
        title="How far over the noise floor counts as busy"
        value={settings.margin_db ?? (type === "wifi_occupancy" ? 6 : 10)}
        {...limitOf(limits, "margin_db")}
        unit="dB"
        onCommit={(margin_db) => onParams({ type, settings: { ...settings, margin_db } })}
      />
    </>
  );
}

import {
  EMPTY,
  ISM_KIND_LABELS,
  latestRecord,
  PANE,
  type PanelProps,
  percentOf,
  TABLE_CELL,
  TABLE_HEAD,
  Unit,
  WIFI_BAND_LABELS,
} from "./panelKit";

function LoadBar({ share }: { share: number }) {
  return (
    <div className="h-2 w-24 overflow-hidden rounded-control bg-line">
      <div className="h-full bg-accent" style={{ width: `${Math.min(100, share * 100)}%` }} />
    </div>
  );
}

export function WifiOccupancyView({ records }: PanelProps<"wifi_occupancy">) {
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

export function IsmSurveyView({ records }: PanelProps<"ism_survey">) {
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
