import { callAudioUrl } from "../lib/api";
import { Fields } from "../sinks/Fields";
import type { LogRow } from "./decoderLog";

export function RowDetail({ row }: { row: LogRow }) {
  return (
    <div className="flex flex-col gap-2">
      {row.clip !== null && (
        <audio
          className="h-8 w-full min-w-0"
          controls
          preload="none"
          src={callAudioUrl(row.clip.url)}
        />
      )}
      <Fields fields={row.fields} />
      {row.fields.length === 0 && row.clip === null && (
        <span className="text-ink-dim">This frame carried nothing beyond its summary.</span>
      )}
    </div>
  );
}
