import { joinFields } from "../decoderViews";
import { EMPTY, formatClock, PANE, type PanelProps } from "./panelKit";

export function ToneView({ records }: PanelProps<"tone">) {
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

export function toneLabel(status: { ctcss_hz?: number | null; dcs_code?: number | null }): string {
  return joinFields(
    status.ctcss_hz == null ? "" : `CTCSS ${status.ctcss_hz.toFixed(1)} Hz`,
    status.dcs_code == null ? "" : `DCS ${String(status.dcs_code).padStart(3, "0")}`,
  );
}
