import { Button } from "../../components/BaseControls";
import { BTN_SM, TABLE_CELL, TABLE_HEAD } from "../../components/controls";
import { Unit } from "../../components/Unit";
import type { TrunkChannel, TrunkChannelEntry } from "../../lib/types";
import { FaceEmpty } from "./NodeShell";
import {
  planSummary,
  trunkChannelSourceHint,
  trunkChannelSourceLabel,
  trunkChannelSourceTone,
  usable,
  withoutChannel,
} from "./trunk";

export function ChannelPlanTable({
  label,
  rows,
  entries,
  found,
  following,
  hint,
  onChange,
}: {
  label: string;
  rows: readonly TrunkChannel[];
  entries: readonly TrunkChannelEntry[];
  found: readonly TrunkChannelEntry[];
  following: ReadonlySet<number>;
  /** What the empty table waits for, if it waits for a setting. */
  hint?: string | null;
  onChange: (entries: TrunkChannelEntry[]) => void;
}) {
  const summary = planSummary(rows);
  return (
    <div className="flex min-h-32 flex-1 flex-col">
      <div className="flex items-center justify-between gap-2 px-2 pt-2">
        <span className="legend">{label}</span>
        {found.length > 0 && (
          <Button type="button" className={BTN_SM} onClick={() => onChange([...entries, ...found])}>
            Keep {found.length} found
          </Button>
        )}
      </div>
      <div className="m-2 flex min-h-0 flex-1 flex-col overflow-y-auto rounded-control border border-line">
        <table className="w-full border-collapse font-mono text-xs">
          <thead className="sticky top-0 bg-panel">
            <tr>
              <th className={`${TABLE_HEAD} w-0 text-right`}>LCN</th>
              <th className={`${TABLE_HEAD} w-0 text-right`}>
                <Unit symbol="MHz" />
              </th>
              <th className={TABLE_HEAD}>Known by</th>
              <th className={TABLE_HEAD}>
                <span className="sr-only">Actions</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((channel) => (
              <tr
                key={channel.logical_channel}
                className={`border-t border-line ${following.has(channel.logical_channel) ? "bg-accent/10" : ""}`}
              >
                <td className={`${TABLE_CELL} text-right text-ink`}>{channel.logical_channel}</td>
                <td
                  className={`${TABLE_CELL} text-right whitespace-nowrap ${
                    usable(channel.source) ? "text-ink" : "text-ink-faint"
                  }`}
                >
                  {(channel.freq_hz / 1e6).toFixed(4)}
                </td>
                <td
                  className={`${TABLE_CELL} ${trunkChannelSourceTone(channel.source)}`}
                  title={trunkChannelSourceHint(channel.source)}
                >
                  {trunkChannelSourceLabel(channel.source)}
                  {channel.source === "learned" && ` ${channel.confidence}%`}
                </td>
                <td className={`${TABLE_CELL} text-right`}>
                  {channel.source === "manual" && (
                    <Button
                      type="button"
                      className={BTN_SM}
                      aria-label={`Forget channel ${channel.logical_channel}`}
                      onClick={() => onChange(withoutChannel(entries, channel.logical_channel))}
                    >
                      Forget
                    </Button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {rows.length === 0 && <FaceEmpty hint={hint ?? undefined} />}
      </div>
      {summary !== null && <p className="px-2 pb-2 text-xs text-ink-dim">{summary}</p>}
    </div>
  );
}
