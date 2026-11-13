import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "../../components/BaseControls";
import { BTN, BTN_SM } from "../../components/controls";
import { ChoiceChip } from "../../components/face/Chips";
import { COLOUR_OPTIONS } from "../../components/plotFrame";
import { type Colormap, DEFAULT_COLORMAP } from "../../gl/surface";
import { nodeAction } from "../../lib/api";
import { processorStatusOf, useArrayStore } from "../../lib/arrays";
import { type ProcessorState, readoutsOf, useProcessorStore } from "../../lib/processors";
import { clearAction, failAction } from "../../lib/refusals";
import type {
  ArrayTuningMode,
  PatchNode,
  ProcessorFacets,
  ProcessorView,
  ViewSpec,
} from "../../lib/types";
import { useNow } from "../../lib/useNow";
import { useWorkspaceContext } from "../context";
import { arrayOf } from "../lanes";
import { NodeForm, StatusGrid } from "./NodeForm";
import { FaceBody, FaceEmpty, NodeShell } from "./NodeShell";
import { ProcessorFooter } from "./ProcessorFooter";
import { ProcessorError } from "./ProcessorHealth";
import {
  ageLabel,
  faceChips,
  NOT_LOADED,
  processorKindOf,
  processorLanes,
  processorStale,
  processorSubtitle,
  tuneArrayEdit,
  tuningAsk,
  tuningAskOf,
} from "./processorFace";
import { BeamPattern } from "./views/BeamPattern";
import { BearingRose } from "./views/BearingRose";
import { EllipseGlyph } from "./views/EllipseGlyph";
import { GRID_STREAMS, SurfacePanel } from "./views/SurfacePanel";
import { ViewTable } from "./views/ViewTable";

const AGE_TICK_MS = 1_000;

type Action = ProcessorFacets["actions"][number];

/** Every array processor's face, built from what its type declares. */
export function ArrayProcessorFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const state = useProcessorStore((store) => store.byNode[node.id]);
  const array = arrayOf(workspace.resolved, node.id);
  const status = useArrayStore((store) => (array === null ? undefined : store.byNode[array]));
  const now = useNow(AGE_TICK_MS);
  const [colormap, setColormap] = useState<Colormap>(DEFAULT_COLORMAP);
  const [picked, setPicked] = useState<number | null>(null);
  const kind = processorKindOf(workspace.context.catalog, node);
  const stale = processorStale(state, now);
  const health = processorStatusOf(status, node.id);
  const ask = tuningAsk(workspace.graph, array, kind.facets.tuning);
  const subtitle = processorSubtitle(workspace.resolved, node.id, status, state, now);
  const surface = surfaceOf(kind.facets.views);
  const retune = (mode: ArrayTuningMode) => {
    workspace.edit((snapshot) => {
      const graph = tuneArrayEdit(snapshot.graph, array, mode);
      return graph === null ? snapshot : { ...snapshot, graph };
    });
    workspace.apply();
  };
  return (
    <NodeShell
      node={node}
      title={kind.title}
      category="tool"
      subtitle={<span className={subtitle.warn ? "text-warn" : undefined}>{subtitle.text}</span>}
    >
      <FaceBody scroll={surface === null}>
        <div className={surface === null ? undefined : "flex min-h-0 flex-1 flex-col"}>
          {!kind.loaded && <FaceEmpty hint={NOT_LOADED} />}
          <DeclaredViews
            node={node.id}
            specs={kind.facets.views}
            state={state}
            dim={stale || state === undefined}
            known={health !== null}
            colormap={colormap}
            picked={picked}
          />
          <StatusGrid readouts={readoutsOf(state)} age={ageLabel(state?.receivedAt, now)} />
          {kind.facets.views.some((spec) => spec.view === "table") && (
            <ViewTable view={state?.view.table ?? null} picked={picked} onPick={setPicked} />
          )}
          <ProcessorError status={health} />
          <div className={surface === null ? undefined : "max-h-56 shrink-0 overflow-y-auto"}>
            {kind.loaded && (
              <NodeForm
                node={node}
                lanes={processorLanes(workspace.resolved, array, status)}
                lead={
                  surface !== null &&
                  GRID_STREAMS.includes(surface) && (
                    <ChoiceChip
                      label="Colours"
                      value={colormap}
                      options={COLOUR_OPTIONS}
                      title="Plot colours"
                      onChange={setColormap}
                    />
                  )
                }
              />
            )}
          </div>
        </div>
      </FaceBody>
      <ProcessorFooter
        status={health}
        chips={faceChips(state, stale, ask)}
        actions={
          ask === null && kind.facets.actions.length === 0 ? undefined : (
            <FaceActions
              node={node.id}
              ask={ask}
              actions={kind.facets.actions}
              idle={state === undefined}
              onRetune={retune}
            />
          )
        }
      />
    </NodeShell>
  );
}

function surfaceOf(specs: readonly ViewSpec[]) {
  for (const spec of specs) {
    if (spec.view === "surface") {
      return spec.stream;
    }
  }
  return null;
}

function DeclaredViews({
  node,
  specs,
  state,
  dim,
  known,
  colormap,
  picked,
}: {
  node: string;
  specs: readonly ViewSpec[];
  state: ProcessorState | undefined;
  dim: boolean;
  known: boolean;
  colormap: Colormap;
  picked: number | null;
}) {
  const view: ProcessorView = state?.view ?? {};
  return specs.map((spec) => {
    switch (spec.view) {
      case "bearing":
        return <BearingRose key={spec.view} view={view.bearing ?? null} dim={dim} />;
      case "pattern":
        return <BeamPattern key={spec.view} view={view.pattern ?? null} dim={dim} />;
      case "ellipse":
        return <EllipseGlyph key={spec.view} view={view.ellipse ?? null} dim={dim} />;
      case "surface":
        return (
          <SurfacePanel
            key={spec.view}
            stream={spec.stream}
            node={node}
            known={known}
            view={view.surface ?? null}
            dim={dim}
            colormap={colormap}
            picked={picked}
            receivedAt={state?.receivedAt}
          />
        );
      case "table":
        return null;
    }
  });
}

function FaceActions({
  node,
  ask,
  actions,
  idle,
  onRetune,
}: {
  node: string;
  ask: ArrayTuningMode | null;
  actions: readonly Action[];
  idle: boolean;
  onRetune: (mode: ArrayTuningMode) => void;
}) {
  return (
    <>
      {ask !== null && (
        <Button
          type="button"
          className={BTN_SM}
          title={tuningAskOf(ask).title}
          onClick={() => onRetune(ask)}
        >
          {tuningAskOf(ask).label}
        </Button>
      )}
      {actions.map((action) => (
        <ActionButton key={action.action} node={node} action={action} idle={idle} />
      ))}
    </>
  );
}

function ActionButton({ node, action, idle }: { node: string; action: Action; idle: boolean }) {
  const mutation = useMutation({
    mutationFn: () => nodeAction(node, { action: "processor", with: action.action }),
    onSuccess: () => clearAction(node, action.label),
    onError: (error) => failAction(node, action.label, error),
  });
  return (
    <Button
      type="button"
      className={BTN}
      title={action.title}
      disabled={mutation.isPending || idle}
      onClick={() => mutation.mutate()}
    >
      {action.label}
    </Button>
  );
}
