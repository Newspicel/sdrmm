import { X } from "lucide-react";
import type { Entity } from "../../lib/types";
import { Fields } from "../../sinks/Fields";
import { entityLabel } from "../../sinks/mapLayers";
import { Button } from "../BaseControls";
import { formatMhz } from "../format";
import { Icon } from "../Icon";

function formatUtc(at: string): string {
  const parsed = Date.parse(at);
  return Number.isNaN(parsed) ? at : `${new Date(parsed).toISOString().slice(11, 19)}Z`;
}

export function EntityCard({
  entity,
  kindName,
  color,
  onClose,
}: {
  entity: Entity;
  kindName: string;
  color: string;
  onClose: () => void;
}) {
  return (
    <div className="absolute inset-x-2 bottom-2 rounded-control border border-line bg-panel/85 md:inset-x-auto md:right-2 md:w-64">
      <div className="flex items-center justify-between gap-2 border-b border-line px-2 py-1">
        <span className="truncate font-mono text-sm" style={{ color }}>
          {entityLabel(entity)}
        </span>
        <span className="legend shrink-0">{kindName}</span>
        <Button
          type="button"
          className="shrink-0 px-1 font-mono text-xs text-ink-dim hover:text-ink"
          onClick={onClose}
          aria-label="Clear selection"
        >
          <Icon glyph={X} size={12} />
        </Button>
      </div>
      <div className="max-h-64 overflow-y-auto px-2 py-1.5">
        <Fields fields={entity.view.fields ?? []} />
      </div>
      <div className="border-t border-line px-2 py-1 font-mono text-2xs tabular-nums text-ink-dim">
        {formatMhz(entity.freq_hz)} · last seen {formatUtc(entity.last_seen)}
      </div>
    </div>
  );
}
