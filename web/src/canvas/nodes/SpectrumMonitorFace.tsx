import { Chips, NumberChip, OptionalNumberChip } from "../../components/face/Chips";
import { Readout, Readouts } from "../../components/face/Readouts";
import type { PatchNode, SpectrumMonitorNode } from "../../lib/types";
import { useWorkspaceNow, useWorkspaceSelect } from "../context";
import { patchNode } from "../graph";
import { Inspect, InspectSection } from "../inspector";
import { FaceBody, NodeShell } from "./NodeShell";
import { ProtocolChip } from "./ProtocolPicker";
import { choiceSummary, protocolGroups } from "./protocols";

const DEFAULT_CONFIDENCE = 0.7;

const toMhz = (hz: number | null | undefined) => (hz == null ? null : hz / 1e6);
const toHz = (mhz: number | null) => (mhz === null ? null : Math.round(mhz * 1e6));

/** The watched band as the face reads it: open edges say so. */
export function bandLabel(start: number | null, stop: number | null): string {
  if (start === null && stop === null) {
    return "All";
  }
  return `${start ?? "…"}–${stop ?? "…"} MHz`;
}

export function SpectrumMonitorFace({ node }: { node: PatchNode }) {
  const now = useWorkspaceNow();
  const families = useWorkspaceSelect((workspace) => workspace.context.catalog.families);
  const channelTypes = useWorkspaceSelect((workspace) => workspace.context.channelTypes);
  if (node.kind !== "spectrum_monitor") return null;
  const edit = (next: Partial<SpectrumMonitorNode>) =>
    now().edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        current.kind === "spectrum_monitor"
          ? { ...current, data: { ...current.data, ...next } }
          : current,
      ),
    }));
  const confidence = Math.round((node.data.min_confidence ?? DEFAULT_CONFIDENCE) * 100);
  const groups = protocolGroups(families ?? [], channelTypes);
  const choice = {
    disabled: node.data.disabled_protocols ?? [],
    unidentified: node.data.report_unidentified ?? true,
  };
  const start = toMhz(node.data.start_hz);
  const stop = toMhz(node.data.stop_hz);
  return (
    <NodeShell node={node} title="Spectrum monitor" category="tool">
      <FaceBody>
        <Readouts>
          <Readout label="Band" title="Frequencies watched">
            {bandLabel(start, stop)}
          </Readout>
          <Readout label="Protocols" title="Protocols decoded">
            {choiceSummary(groups, choice)}
          </Readout>
          <Readout label="Min" title="Minimum confidence">
            {`${confidence} %`}
          </Readout>
        </Readouts>
      </FaceBody>
      <Inspect node={node.id}>
        <InspectSection>
          <Chips inset="none">
            <ProtocolChip
              groups={groups}
              choice={choice}
              onChange={(next) =>
                edit({
                  disabled_protocols: [...next.disabled],
                  report_unidentified: next.unidentified,
                })
              }
            />
            <OptionalNumberChip
              label="Start"
              title="Lowest frequency, empty for all"
              placeholder="all"
              value={start}
              unit="MHz"
              min={0}
              step={0.001}
              onCommit={(mhz) => edit({ start_hz: toHz(mhz) })}
            />
            <OptionalNumberChip
              label="Stop"
              title="Highest frequency, empty for all"
              placeholder="all"
              value={stop}
              unit="MHz"
              min={0}
              step={0.001}
              onCommit={(mhz) => edit({ stop_hz: toHz(mhz) })}
            />
            <NumberChip
              label="Min"
              title="Minimum confidence. Signals identified below it are ignored; 0 accepts all"
              value={confidence}
              unit="%"
              min={0}
              max={100}
              step={5}
              onCommit={(value) => edit({ min_confidence: value / 100 })}
            />
          </Chips>
        </InspectSection>
      </Inspect>
    </NodeShell>
  );
}
