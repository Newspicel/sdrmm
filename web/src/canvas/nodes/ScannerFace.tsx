import { ScannerPanel } from "../../components/ScannerPanel";
import type { PatchNode, PatchNodeOf, ScanSettings } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { defaultScannerSettings } from "../newNode";
import { controlledOf } from "../workspaceDevice";
import { NodeShell } from "./NodeShell";
import { SWEEP_WANTS, useUnwired } from "./needs";

export function ScannerFace({ node }: { node: PatchNode }) {
  if (node.kind !== "scanner") {
    return null;
  }
  return <ScannerNodeFace node={node} />;
}

function ScannerNodeFace({ node }: { node: PatchNodeOf<"scanner"> }) {
  const workspace = useWorkspaceContext();
  const decoder = controlledOf(workspace, node.id);
  const unwired = useUnwired(node.id, SWEEP_WANTS);
  const settings = node.data?.settings ?? defaultScannerSettings(workspace.context.catalog);
  const remember = (next: ScanSettings): void => {
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        current.kind === "scanner"
          ? { ...current, data: { ...current.data, settings: next } }
          : current,
      ),
    }));
  };
  return (
    <NodeShell node={node} title="Scanner" category="tool">
      <ScannerPanel
        node={node.id}
        active={decoder?.set ?? null}
        channel={decoder?.channel ?? null}
        unwired={unwired}
        settings={settings}
        onSettings={remember}
      />
    </NodeShell>
  );
}
