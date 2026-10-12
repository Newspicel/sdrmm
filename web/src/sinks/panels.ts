import type { ReactNode } from "react";
import type { EntityOf, SinkGroup } from "../lib/sinks";
import type { DecodedRecordOf, DecoderKind } from "../lib/types";

export interface PanelProps<K extends DecoderKind> {
  entities: readonly EntityOf<K>[];
  records: readonly DecodedRecordOf<K>[];
}

export type Panels = { [K in DecoderKind]?: (props: PanelProps<K>) => ReactNode };

export function hasPanel(panels: Panels, kind: DecoderKind): boolean {
  return panels[kind] !== undefined;
}

export function renderPanel(panels: Panels, group: SinkGroup): ReactNode | null {
  const panel = panels[group.kind] as ((props: PanelProps<DecoderKind>) => ReactNode) | undefined;
  if (panel === undefined) {
    return null;
  }
  return panel({ entities: group.entities, records: group.records });
}
