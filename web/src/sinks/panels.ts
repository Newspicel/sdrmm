import { createElement, type ReactNode } from "react";
import type { EntityOf, SinkGroup } from "../lib/sinks";
import type { DecodedRecordOf, DecoderKind } from "../lib/types";

export interface PanelProps<K extends DecoderKind> {
  entities: readonly EntityOf<K>[];
  records: readonly DecodedRecordOf<K>[];
}

export type Panels = { [K in DecoderKind]?: (props: PanelProps<K>) => ReactNode };

/// Mounts the panel as its own component, never calls it, so its hooks stay its own and a
/// group appearing or vanishing does not change the hook count of the view around it.
export function renderPanel(panels: Panels, group: SinkGroup): ReactNode | null {
  const panel = panels[group.kind] as ((props: PanelProps<DecoderKind>) => ReactNode) | undefined;
  if (panel === undefined) {
    return null;
  }
  return createElement(panel, { entities: group.entities, records: group.records });
}
