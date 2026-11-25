import { Chips, ToggleChip } from "../../components/face/Chips";
import { HuntPanel } from "../../components/HuntPanel";
import { sweptBy } from "../../lib/resolved";
import type { PatchNode, PatchNodeOf } from "../../lib/types";
import { useWorkspaceNow, useWorkspaceSelect } from "../context";
import { patchNode } from "../graph";
import { Inspect, InspectSection } from "../inspector";
import { hasFeed } from "../lanes";
import { huntSweepOf } from "../newNode";
import { controlledOf } from "../workspaceDevice";
import { HuntSweepChips } from "./HuntSweep";
import { NodeShell } from "./NodeShell";
import { SWEEP_WANTS, useUnwired } from "./needs";

export function HuntFace({ node }: { node: PatchNode }) {
  if (node.kind !== "hunt") {
    return null;
  }
  return <HuntNodeFace node={node} />;
}

function HuntNodeFace({ node }: { node: PatchNodeOf<"hunt"> }) {
  const now = useWorkspaceNow();
  const decoder = useWorkspaceSelect((workspace) => controlledOf(workspace, node.id));
  const catalog = useWorkspaceSelect((workspace) => workspace.context.catalog);
  const sweep = huntSweepOf(node, catalog);
  const unwired = useUnwired(node.id, SWEEP_WANTS);
  const scanning = useWorkspaceSelect(
    (workspace) => decoder !== null && sweptBy(workspace.resolved, decoder.node, node.id),
  );
  const positionWired = useWorkspaceSelect((workspace) =>
    hasFeed(workspace.resolved, node.id, "position"),
  );
  const remember = (data: Partial<PatchNodeOf<"hunt">["data"]>): void => {
    now().edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        current.kind === "hunt" ? { ...current, data: { ...current.data, ...data } } : current,
      ),
    }));
  };
  const clicks = node.data.clicks ?? true;
  return (
    <NodeShell node={node} title="Signal hunt" category="tool">
      <Inspect node={node.id}>
        <InspectSection>
          <Chips inset="none">
            <ToggleChip
              label="Clicks"
              title="Geiger clicks"
              on={clicks}
              onChange={(next) => remember({ clicks: next })}
            />
          </Chips>
        </InspectSection>
        {positionWired && sweep !== null && (
          <InspectSection label="Sweep">
            <Chips inset="none">
              <HuntSweepChips
                params={sweep}
                edit={(next) => remember({ sweep: { ...sweep, ...next } })}
              />
            </Chips>
          </InspectSection>
        )}
      </Inspect>
      <HuntPanel
        node={node.id}
        target={decoder}
        clicks={clicks}
        unwired={unwired}
        scanning={scanning}
        positionWired={positionWired}
      />
    </NodeShell>
  );
}
