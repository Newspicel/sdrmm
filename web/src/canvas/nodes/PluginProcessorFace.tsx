import { processorStatusOf, useArrayStore } from "../../lib/arrays";
import { isStale, readoutsOf, useProcessorStore } from "../../lib/processors";
import type { PatchNode } from "../../lib/types";
import { useNow } from "../../lib/useNow";
import { useWorkspaceContext } from "../context";
import { arrayOf } from "../lanes";
import { processorTypeOf } from "../newNode";
import { NodeForm, StatusGrid } from "./NodeForm";
import { FaceBody, FaceEmpty, NodeShell } from "./NodeShell";
import { ProcessorFooter } from "./ProcessorFooter";
import { ProcessorError } from "./ProcessorHealth";
import { ageLabel, processorLanes, processorSubtitle } from "./processorFace";

const AGE_TICK_MS = 1_000;
const REPORT_MS = 1_000;
export const NOT_LOADED = "Not loaded";
const FALLBACK_TITLE = "Processor";

/** A plugin's array processor: its form and readouts, whatever its type. */
export function PluginProcessorFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const state = useProcessorStore((store) => store.byNode[node.id]);
  const array = arrayOf(workspace.resolved, node.id);
  const status = useArrayStore((store) => (array === null ? undefined : store.byNode[array]));
  const now = useNow(AGE_TICK_MS);
  if (node.kind !== "processor") {
    return null;
  }
  const typeId = node.data?.processor_type ?? "";
  const type = processorTypeOf(workspace.context.catalog, typeId);
  const stale = state !== undefined && isStale(state.receivedAt, now, REPORT_MS);
  const processor = processorStatusOf(status, node.id);
  return (
    <NodeShell
      node={node}
      title={type?.name ?? (typeId === "" ? FALLBACK_TITLE : typeId)}
      category="tool"
      subtitle={processorSubtitle(workspace.resolved, node.id, status, state, now, REPORT_MS)}
    >
      <FaceBody>
        {type === undefined && <FaceEmpty hint={NOT_LOADED} />}
        <div className={stale ? "opacity-45" : ""}>
          <StatusGrid readouts={readoutsOf(state)} age={ageLabel(state?.receivedAt, now)} />
        </div>
        {type !== undefined && (
          <NodeForm node={node} lanes={processorLanes(workspace.resolved, array, status)} />
        )}
        <ProcessorError status={processor} />
      </FaceBody>
      <ProcessorFooter status={processor} />
    </NodeShell>
  );
}
