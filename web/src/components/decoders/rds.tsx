import { EMPTY, PANE, type PanelProps } from "./panelKit";

export function RdsView({ records }: PanelProps<"rds">) {
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

import { type DecodedRecordOf, type RdsUpdate } from "../../lib/types";
import { formatHz } from "../format";

export function rdsPicture(records: readonly DecodedRecordOf<"rds">[]): RdsUpdate | null {
  const current = sameStation(records);
  if (current.length === 0) {
    return null;
  }
  const merged = current.reduceRight<Record<string, unknown>>((acc, r) => {
    for (const [key, value] of Object.entries(r.event.data)) {
      if (value != null) {
        acc[key] = value;
      }
    }
    return acc;
  }, {});
  return merged as unknown as RdsUpdate;
}

export function sameStation(
  records: readonly DecodedRecordOf<"rds">[],
): readonly DecodedRecordOf<"rds">[] {
  const newest = records[0]?.event.data;
  if (newest == null) {
    return [];
  }
  let groups = newest.groups;
  let end = records.length;
  for (let i = 1; i < records.length; i += 1) {
    const older = records[i]?.event.data;
    if (older == null) {
      break;
    }
    const restarted = older.groups > groups;
    const retuned = older.pi != null && newest.pi != null && older.pi !== newest.pi;
    if (restarted || retuned) {
      end = i;
      break;
    }
    groups = older.groups;
  }
  return records.slice(0, end);
}

export type RdsQualityLabel = "no lock" | "good" | "fair" | "poor";

export interface RdsQuality {
  groups: number;
  blockErrors: number;
  errorRate: number;
  label: RdsQualityLabel;
  className: string;
}

export function rdsQuality(update: RdsUpdate): RdsQuality {
  const groups = update.groups;
  const blockErrors = update.block_errors;
  const blocks = Math.max(update.blocks, groups * 4 + blockErrors);
  const errorRate = blocks === 0 ? 0 : blockErrors / blocks;
  if (groups === 0) {
    return { groups, blockErrors, errorRate, label: "no lock", className: "text-danger" };
  }
  if (errorRate < 0.02) {
    return { groups, blockErrors, errorRate, label: "good", className: "text-accent" };
  }
  if (errorRate < 0.1) {
    return { groups, blockErrors, errorRate, label: "fair", className: "text-ink" };
  }
  return { groups, blockErrors, errorRate, label: "poor", className: "text-danger" };
}

export function ptyLabel(update: RdsUpdate): string {
  if (update.pty_name != null && update.pty_name !== "") {
    return update.pty_name;
  }
  return update.pty == null ? "-" : `PTY ${update.pty}`;
}

export function formatAltFreqs(hz: readonly number[] | undefined): string[] {
  return (hz ?? []).toSorted((a, b) => a - b).map(formatHz);
}
