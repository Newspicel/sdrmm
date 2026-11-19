import { Button } from "../../components/BaseControls";
import { FACE_BTN, FACE_BTN_DANGER } from "../../components/controls";
import { inTuningRange, tuningRange } from "../../components/dial";
import { dialId, FrequencyDial } from "../../components/FrequencyDial";
import { FaceFault } from "../../components/face/Fault";
import { formatMhz } from "../../components/format";
import { RadioSettings } from "../../components/RadioSettings";
import { TuneTo } from "../../components/TuneTo";
import type { PatchNode } from "../../lib/types";
import { useDevicePatch } from "../../lib/useDevicePatch";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { Inspect, InspectSection } from "../inspector";
import { FaceBody, FaceFooter, NodeShell, useFaceActive } from "./NodeShell";
import { SourceHealth } from "./SourceHealth";

export function SignalGenFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const { applyPatch } = useDevicePatch();
  const active = useFaceActive();
  const set = workspace.devices.get(node.id) ?? null;

  const hold = (stopped: boolean): void =>
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (stored) =>
        stored.kind === "device" ? { ...stored, data: { ...stored.data, stopped } } : stored,
      ),
    }));

  const start = (): void => {
    hold(false);
    workspace.apply();
  };

  const stop = (): void => hold(true);

  if (set === null) {
    return (
      <NodeShell node={node} title="Signal generator" category="source">
        <FaceFooter>
          <Button
            type="button"
            className={FACE_BTN}
            title="Generate a decodable signal to try a decoder without a radio"
            onClick={start}
          >
            Start
          </Button>
        </FaceFooter>
      </NodeShell>
    );
  }

  const range = tuningRange(set.capabilities);
  const centerHz = set.settings.center_hz ?? 0;

  return (
    <NodeShell
      node={node}
      title="Signal generator"
      category="source"
      subtitle={set.status === "error" ? <span className="text-danger">error</span> : undefined}
    >
      <FaceBody>
        <div className="@container flex min-w-0 items-center gap-1 p-2">
          <FrequencyDial
            id={dialId(node.id, 0)}
            hz={centerHz}
            range={range}
            wheelTunes={active}
            onTune={(hz) => applyPatch(node.id, { center_hz: hz })}
          />
          <span className="ml-auto flex shrink-0 items-center gap-1">
            <TuneTo
              title="Type the frequency to generate at"
              hz={centerHz}
              hint={`Reaches ${formatMhz(range.min)} to ${formatMhz(range.max)}`}
              resolve={(entered) => inTuningRange(entered, range)}
              onTune={(hz) => applyPatch(node.id, { center_hz: hz })}
            />
          </span>
        </div>
        {set.error != null && <FaceFault message={set.error} />}
      </FaceBody>
      <Inspect node={node.id}>
        <InspectSection label="Radio">
          <RadioSettings node={node.id} active={set} />
        </InspectSection>
      </Inspect>
      <FaceFooter>
        <SourceHealth set={set} />
        <Button
          type="button"
          className={FACE_BTN_DANGER}
          title="Stop generating and free the node: the wires stay drawn"
          onClick={stop}
        >
          Stop
        </Button>
      </FaceFooter>
    </NodeShell>
  );
}
