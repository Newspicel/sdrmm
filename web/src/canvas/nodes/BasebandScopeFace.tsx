import { useState } from "react";
import type { PatchNode } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { basebandSourceOf } from "../lanes";
import { BasebandView } from "./BasebandView";
import { FaceBody, NodeShell } from "./NodeShell";
import { readPalette } from "./scopePalette";

export function BasebandScopeFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const [palette] = useState(readPalette);
  const tap = basebandSourceOf(workspace.resolved, workspace.deviceSets, node.id);
  return (
    <NodeShell node={node} title="Baseband scope" category="output">
      <FaceBody scroll={false}>
        {tap === null ? (
          <div className="flex h-full items-center justify-center bg-plot-bg legend font-mono text-2xs text-plot-ink-dim">
            Wire a channel's baseband
          </div>
        ) : (
          <BasebandView
            key={`${tap.deviceSet}:${tap.channel.id}`}
            node={node.id}
            colormap={palette}
          />
        )}
      </FaceBody>
    </NodeShell>
  );
}
