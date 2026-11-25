import { X } from "lucide-react";
import { Button, Input } from "../../components/BaseControls";
import { BTN, FIELD, ICON_BTN_SM_DANGER } from "../../components/controls";
import { Chips } from "../../components/face/Chips";
import { FaceFault } from "../../components/face/Fault";
import { TextChip } from "../../components/face/TextChip";
import { Icon } from "../../components/Icon";
import { SegmentedToggles } from "../../components/Segmented";
import { SCHEDULE_LIMITS } from "../../lib/limits";
import { useScheduleStore } from "../../lib/schedule";
import type { PatchNode, PatchNodeOf, ScheduleNode, ScheduleWindow } from "../../lib/types";
import { useNow } from "../../lib/useNow";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { Inspect, InspectSection } from "../inspector";
import { FaceBody, FaceEmpty, NodeShell } from "./NodeShell";
import { useUnwired, type Want } from "./needs";
import {
  clockText,
  daysOn,
  newWindow,
  parsedClock,
  stateLine,
  WEEKDAYS,
  withDays,
} from "./schedule";

const NOW_EVERY_MS = 30_000;
const WANTS: readonly Want[] = [{ port: "control", out: true }];

export function ScheduleFace({ node }: { node: PatchNode }) {
  if (node.kind !== "schedule") {
    return null;
  }
  return <ScheduleNodeFace node={node} />;
}

function ScheduleNodeFace({ node }: { node: PatchNodeOf<"schedule"> }) {
  const workspace = useWorkspaceContext();
  const status = useScheduleStore((store) => store.byNode[node.id]) ?? null;
  const now = useNow(NOW_EVERY_MS);
  const unwired = useUnwired(node.id, WANTS);
  const windows = node.data.windows ?? [];
  const edit = (next: ScheduleNode): void =>
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        current.kind === "schedule" ? { ...current, data: next } : current,
      ),
    }));
  const zoned = (zone: string): ScheduleNode => (zone === "" ? { windows } : { windows, zone });
  const setWindows = (next: ScheduleWindow[]): void =>
    edit({ ...zoned(node.data.zone ?? ""), windows: next });
  const full = windows.length >= SCHEDULE_LIMITS.windows;
  return (
    <NodeShell
      node={node}
      title="Schedule"
      category="tool"
      subtitle={stateLine(status, windows.length, now)}
    >
      <Inspect node={node.id}>
        <InspectSection label="Windows">
          {windows.map((window, index) => (
            <WindowRow
              // biome-ignore lint/suspicious/noArrayIndexKey: windows have no identity of their own
              key={index}
              window={window}
              onChange={(next) =>
                setWindows(windows.map((held, at) => (at === index ? next : held)))
              }
              onRemove={() => setWindows(windows.filter((_, at) => at !== index))}
            />
          ))}
          <Button
            type="button"
            className={`${BTN} self-start`}
            disabled={full}
            title={full ? `At most ${SCHEDULE_LIMITS.windows}` : undefined}
            onClick={() => setWindows([...windows, newWindow()])}
          >
            Add window
          </Button>
        </InspectSection>
        <InspectSection label="Clock">
          <Chips inset="none">
            <TextChip
              label="Zone"
              title="IANA time zone, such as Europe/Berlin. Empty keeps the server clock"
              value={node.data.zone ?? ""}
              placeholder="server"
              maxLength={SCHEDULE_LIMITS.zone_len}
              onCommit={(zone) => edit(zoned(zone.trim()))}
            />
          </Chips>
        </InspectSection>
      </Inspect>
      <FaceBody>
        {unwired || status?.error == null ? <FaceEmpty /> : <FaceFault message={status.error} />}
      </FaceBody>
    </NodeShell>
  );
}

function WindowRow({
  window,
  onChange,
  onRemove,
}: {
  window: ScheduleWindow;
  onChange: (next: ScheduleWindow) => void;
  onRemove: () => void;
}) {
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      <ClockInput
        label="On at"
        minutes={window.start_min}
        onCommit={(start_min) => onChange({ ...window, start_min })}
      />
      <span className="text-ink-faint">to</span>
      <ClockInput
        label="Off at"
        minutes={window.stop_min}
        onCommit={(stop_min) => onChange({ ...window, stop_min })}
      />
      <SegmentedToggles
        label="Days it opens"
        values={daysOn(window)}
        options={WEEKDAYS}
        onChange={(days) => onChange(withDays(window, days))}
      />
      <Button
        type="button"
        aria-label="Remove window"
        className={ICON_BTN_SM_DANGER}
        onClick={onRemove}
      >
        <Icon glyph={X} size={12} />
      </Button>
    </div>
  );
}

function ClockInput({
  label,
  minutes,
  onCommit,
}: {
  label: string;
  minutes: number;
  onCommit: (minutes: number) => void;
}) {
  return (
    <Input
      type="time"
      className={`${FIELD} w-24 font-mono tabular-nums`}
      aria-label={label}
      title={label}
      value={clockText(minutes)}
      onChange={(event) => {
        const parsed = parsedClock(event.target.value);
        if (parsed !== null && parsed !== minutes) {
          onCommit(parsed);
        }
      }}
    />
  );
}
