import { Fragment } from "react";
import { callAudioUrl } from "../lib/api";
import { BroadcastDataView } from "./BroadcastDataView";
import { eventDetail } from "./decoderDetail";
import type { LogRow } from "./decoderLog";

export function RowDetail({ row }: { row: LogRow }) {
  const detail = eventDetail(row.event);
  return (
    <div className="flex flex-col gap-2">
      {(row.event.kind === "call" || row.event.kind === "transmission") &&
        row.event.data.audio != null && (
          <audio
            className="h-8 w-full min-w-0"
            controls
            preload="none"
            src={callAudioUrl(row.event.data.audio.url)}
          />
        )}
      {row.event.kind === "broadcast_data" && <BroadcastDataView data={row.event.data} />}
      {detail.fields.length > 0 && (
        <dl className="grid grid-cols-[max-content_1fr] gap-x-3 gap-y-0.5">
          {detail.fields.map(([label, value]) => (
            <Fragment key={label}>
              <dt className="text-ink-dim">{label}</dt>
              <dd className="min-w-0 break-all text-ink">{value}</dd>
            </Fragment>
          ))}
        </dl>
      )}
      {detail.body !== null && (
        <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-words rounded border border-line bg-panel px-2 py-1.5 text-ink">
          {detail.body}
        </pre>
      )}
      {detail.fields.length === 0 && detail.body === null && (
        <span className="text-ink-dim">This frame carried nothing beyond its summary.</span>
      )}
    </div>
  );
}
