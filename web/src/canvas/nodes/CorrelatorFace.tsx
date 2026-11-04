import { useState } from "react";
import { ChoiceChip } from "../../components/face/Chips";
import { Readout, Readouts } from "../../components/face/Readouts";
import { processorStatusOf, useArrayStore } from "../../lib/arrays";
import { isStale, readingOf, readoutsOf, useProcessorStore } from "../../lib/processors";
import type { Baseline, CorrelatorReading, PatchNode } from "../../lib/types";
import { useNow } from "../../lib/useNow";
import { useWorkspaceContext } from "../context";
import { arrayOf } from "../lanes";
import { CorrelatorPlots, FringeLine } from "./CorrelatorPlots";
import { baselineLabel, baselineOptions, baselineText, withFringe } from "./correlator";
import { NodeForm, nodeSettings, StatusGrid } from "./NodeForm";
import { FaceBody, NodeShell } from "./NodeShell";
import { ProcessorFooter } from "./ProcessorFooter";
import { ProcessorError } from "./ProcessorHealth";
import { ageLabel, processorSubtitle } from "./processorFace";

const AGE_TICK_MS = 1_000;

interface Fringe {
  key: string;
  index: number;
  history: readonly number[];
}

function useFringe(reading: CorrelatorReading | null, index: number): readonly number[] {
  const [fringe, setFringe] = useState<Fringe>({ key: "", index, history: [] });
  const baseline = reading?.baselines[index];
  const key = `${index}:${reading?.at ?? ""}`;
  if (baseline !== undefined && fringe.key !== key) {
    const phase = baseline.phase_deg ?? 0;
    const history = fringe.index === index ? withFringe(fringe.history, phase) : [phase];
    setFringe({ key, index, history });
    return history;
  }
  return fringe.index === index ? fringe.history : [];
}

export function CorrelatorFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const state = useProcessorStore((store) => store.byNode[node.id]);
  const array = arrayOf(workspace.resolved, node.id);
  const status = useArrayStore((store) => (array === null ? undefined : store.byNode[array]));
  const now = useNow(AGE_TICK_MS);
  const [chosen, setChosen] = useState(0);
  const reading = readingOf(state, "correlator");
  const index = chosen < (reading?.baselines.length ?? 0) ? chosen : 0;
  const fringe = useFringe(reading, index);
  if (node.kind !== "correlator") {
    return null;
  }
  const integrateS = Number(nodeSettings(workspace.context.catalog, node)?.integrate_s ?? 1);
  const period = Math.max(1_000, integrateS * 1_000);
  const stale = state !== undefined && isStale(state.receivedAt, now, period);
  const baselines = reading?.baselines ?? [];
  const processor = processorStatusOf(status, node.id);
  return (
    <NodeShell
      node={node}
      title="Correlator"
      category="tool"
      subtitle={processorSubtitle(workspace.resolved, node.id, status, state, now, period)}
    >
      <FaceBody scroll={false}>
        <div className="flex min-h-0 flex-1 flex-col">
          <CorrelatorPlots
            node={node.id}
            known={processor !== null}
            index={index}
            dim={stale || reading === null}
          />
          <NodeForm
            node={node}
            lead={
              baselines.length > 0 && (
                <ChoiceChip
                  label="Baseline"
                  title="Baseline shown"
                  value={index}
                  options={baselineOptions(baselines)}
                  onChange={setChosen}
                />
              )
            }
          />
          <BaselineLine baseline={baselines[index]} fringe={fringe} />
          <StatusGrid readouts={readoutsOf(state)} age={ageLabel(state?.receivedAt, now)} />
          <ProcessorError status={processor} />
        </div>
      </FaceBody>
      <ProcessorFooter status={processor} />
    </NodeShell>
  );
}

function BaselineLine({
  baseline,
  fringe,
}: {
  baseline: Baseline | undefined;
  fringe: readonly number[];
}) {
  if (baseline === undefined) {
    return null;
  }
  return (
    <div className="flex shrink-0 items-center gap-2 border-t border-line pr-2">
      <Readouts ruled={false} className="min-w-0 flex-1">
        <Readout label={baselineLabel(baseline)} title="Coherence, phase, delay, SNR">
          {baselineText(baseline)}
        </Readout>
      </Readouts>
      <FringeLine history={fringe} />
    </div>
  );
}
