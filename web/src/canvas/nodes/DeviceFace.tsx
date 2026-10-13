import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link2, Radar } from "lucide-react";
import type { ReactNode } from "react";
import { Button } from "../../components/BaseControls";
import { BTN_PRIMARY, BTN_QUIET_DANGER, BTN_SM, ICON_BTN } from "../../components/controls";
import { inTuningRange, isTunable, tuningRange } from "../../components/dial";
import { dialId, FrequencyDial } from "../../components/FrequencyDial";
import { FaceFault } from "../../components/face/Fault";
import { Stat } from "../../components/face/Stats";
import { formatMhz } from "../../components/format";
import { Icon } from "../../components/Icon";
import { laneLayout } from "../../components/laneRows";
import { DeviceChoices } from "../../components/OpenRadio";
import { RadioSettings } from "../../components/RadioSettings";
import { Tip } from "../../components/Tip";
import { TuneTo } from "../../components/TuneTo";
import { TuningLock } from "../../components/TuningLock";
import { addNetworkRadio, devicesQuery, STATE_KEY } from "../../lib/api";
import { toastError } from "../../lib/toasts";
import type { DeviceInfo, DeviceRef, DeviceSet, PatchNode, PatchNodeOf } from "../../lib/types";
import { useRadioTune } from "../../lib/useRadioTune";
import { useWorkspaceContext } from "../context";
import { patchNode, rxStreamCount, streamPort } from "../graph";
import { dialHold, type LaneHold, laneHolds } from "./arrayNode";
import {
  allAutoTuning,
  allLocked,
  autoTuning,
  bondSaid,
  claimedDevices,
  clippingSaid,
  deviceRefOf,
  faultSaid,
  type Hearing,
  hearing,
  lanesMerged,
  lockAll,
  lockStream,
  lossSaid,
  refLabel,
  refMatches,
  refusalSaid,
  type TunerDial,
  tunerDials,
} from "./deviceNode";
import { MakeArrayButton, offersMakeArray } from "./MakeArrayButton";
import { FaceBody, FaceFooter, NodeShell, useFaceActive } from "./NodeShell";
import { SourceHealth } from "./SourceHealth";

type DeviceNodeData = PatchNodeOf<"device">["data"];

function AutoTuning({
  node,
  set,
  stream,
}: {
  node: string;
  set: DeviceSet;
  stream: number | "all";
}) {
  const { setTuning, setTuningAll } = useRadioTune();
  const auto = stream === "all" ? allAutoTuning(set) : autoTuning(set, stream);
  const flip = (): void => {
    const next = auto ? "manual" : "auto";
    if (stream === "all") {
      setTuningAll({ node, set }, next);
    } else {
      setTuning({ node, set }, stream, next);
    }
  };
  return (
    <Tip
      text={auto ? "Auto mode: following decoders" : "Auto mode: follow decoders"}
      render={
        <Button
          type="button"
          className={`${ICON_BTN} ${auto ? "bg-accent/15" : ""}`}
          aria-label={auto ? "Tune by hand" : "Follow the decoders"}
          aria-pressed={auto}
          onClick={flip}
        />
      }
    >
      <span className={auto ? "flex text-accent" : "flex"}>
        <Icon glyph={Radar} size={16} />
      </span>
    </Tip>
  );
}

function LinkLanes({ linked, onLink }: { linked: boolean; onLink: (linked: boolean) => void }) {
  return (
    <Tip
      text={linked ? "Lanes tune together. Click to tune each lane" : "Tune all lanes together"}
      render={
        <Button
          type="button"
          className={`${ICON_BTN} ${linked ? "bg-accent/15" : ""}`}
          aria-label={linked ? "Tune lanes one by one" : "Tune all lanes together"}
          aria-pressed={linked}
          onClick={() => onLink(!linked)}
        />
      }
    >
      <span className={linked ? "flex text-accent" : "flex"}>
        <Icon glyph={Link2} size={16} />
      </span>
    </Tip>
  );
}

interface TunerProps {
  node: string;
  set: DeviceSet;
  lockedStreams: readonly number[];
  onLock: (locked: number[]) => void;
  split: boolean;
  onSplit: (split: boolean) => void;
  holds: ReadonlyMap<number, LaneHold>;
}

function HeldBadge({ hold }: { hold: LaneHold }) {
  const workspace = useWorkspaceContext();
  return (
    <Button
      type="button"
      className={`${BTN_SM} text-hue-indigo`}
      title={`Tuned by ${hold.label}`}
      onClick={() => workspace.select(hold.array)}
    >
      <Icon glyph={Link2} size={12} />
      Array
    </Button>
  );
}

function DialRow({
  node,
  set,
  dial,
  locked,
  onLock,
  tools,
  hold,
  onTune,
}: {
  node: string;
  set: DeviceSet;
  dial: TunerDial;
  locked: boolean;
  onLock: (locked: boolean) => void;
  tools: ReactNode;
  hold: LaneHold | null;
  onTune: (hz: number) => void;
}) {
  const active = useFaceActive();
  const range = tuningRange(set.capabilities);
  const pinned = !isTunable(range);
  const held = pinned || locked || hold !== null;
  const free = !pinned && hold === null;
  return (
    <div
      className="@container flex min-w-0 items-center gap-2"
      title={hold === null ? undefined : `Tuned by ${hold.label}`}
    >
      <FrequencyDial
        id={dialId(node, dial.stream)}
        hz={dial.hz}
        range={range}
        disabled={held}
        wheelTunes={active}
        onTune={onTune}
      />
      <span className="ml-auto flex shrink-0 items-center gap-1">
        {hold !== null && <HeldBadge hold={hold} />}
        {free && (
          <TuneTo
            title="Type a frequency"
            hz={dial.hz}
            hint={`Reaches ${formatMhz(range.min)} to ${formatMhz(range.max)}`}
            resolve={(entered) => inTuningRange(entered, range)}
            disabled={held}
            onTune={onTune}
          />
        )}
        {free && tools}
        {free && (
          <TuningLock locked={locked} held="Tuning locked" free="Lock tuning" onLock={onLock} />
        )}
      </span>
    </div>
  );
}

function LaneDial({
  node,
  set,
  dial,
  locked,
  hold,
}: {
  node: string;
  set: DeviceSet;
  dial: TunerDial;
  locked: boolean;
  hold: LaneHold | null;
}) {
  const { tuneRadio } = useRadioTune();
  const active = useFaceActive();
  const range = tuningRange(set.capabilities);
  const held = !isTunable(range) || locked || hold !== null;
  const tune = (hz: number): void => tuneRadio({ node, set }, dial.stream, hz);
  return (
    <div
      className="flex h-7 items-center gap-2"
      title={hold === null ? undefined : `Tuned by ${hold.label}`}
    >
      <span className="w-14 shrink-0 truncate font-mono text-2xs text-hue-blue">{dial.port}</span>
      <div className="@container w-52 min-w-0">
        <FrequencyDial
          id={dialId(node, dial.stream)}
          hz={dial.hz}
          range={range}
          disabled={held}
          wheelTunes={active}
          onTune={tune}
        />
      </div>
      <span className="ml-auto flex shrink-0 items-center gap-1">
        {hold === null ? (
          <TuneTo
            title={`Type a frequency for ${dial.port}`}
            hz={dial.hz}
            hint={`Reaches ${formatMhz(range.min)} to ${formatMhz(range.max)}`}
            resolve={(entered) => inTuningRange(entered, range)}
            disabled={held}
            onTune={tune}
          />
        ) : (
          <HeldBadge hold={hold} />
        )}
      </span>
    </div>
  );
}

function Tuner(props: TunerProps) {
  const { node, set, lockedStreams, onLock, split, onSplit, holds } = props;
  const { tuneRadio, tuneAll } = useRadioTune();
  const caps = set.capabilities;
  const merged = lanesMerged(set);
  const dial = tunerDials(set)[0];
  const locked = merged ? allLocked(lockedStreams, caps) : lockedStreams.includes(0);
  const lock = (held: boolean): void =>
    onLock(merged ? lockAll(caps, held) : lockStream(lockedStreams, 0, held));
  const hold = dialHold(holds, 0, false);
  const link = merged && (
    <LinkLanes
      linked={!split}
      onLink={(linked) => {
        onSplit(!linked);
        if (linked && dial !== undefined) {
          tuneAll({ node, set }, dial.hz);
        }
      }}
    />
  );
  if (merged && split) {
    return (
      <div className="flex h-7 items-center gap-2">
        <span className="legend">Per lane</span>
        <span className="ml-auto flex shrink-0 items-center gap-1">
          {hold === null ? (
            <>
              {link}
              <AutoTuning node={node} set={set} stream="all" />
              <TuningLock locked={locked} held="Tuning locked" free="Lock tuning" onLock={lock} />
            </>
          ) : (
            <HeldBadge hold={hold} />
          )}
        </span>
      </div>
    );
  }
  if (dial === undefined) {
    return null;
  }
  return (
    <div>
      <DialRow
        node={node}
        set={set}
        dial={dial}
        locked={locked}
        hold={hold}
        onLock={lock}
        onTune={(hz) => (merged ? tuneAll({ node, set }, hz) : tuneRadio({ node, set }, 0, hz))}
        tools={
          <>
            {link}
            <AutoTuning node={node} set={set} stream={merged ? "all" : 0} />
          </>
        }
      />
    </div>
  );
}

const TONE: Record<Hearing["tone"], string> = {
  ok: "text-ok",
  warn: "text-warn",
  danger: "text-danger",
};

const INSIDE = "Decoders wired to this radio that sit inside its window";

function Heard({ set }: { set: DeviceSet }) {
  const heard = hearing(set);
  return (
    <span
      role="status"
      className={TONE[heard.tone]}
      title={
        heard.tone === "ok"
          ? INSIDE
          : `${INSIDE}. Raise the sample rate, or move the ones it misses to another radio.`
      }
    >
      {heard.heard}/{heard.total}
    </span>
  );
}

function Fault({ set }: { set: DeviceSet }) {
  const said = faultSaid(set);
  return <FaceFault message={said ?? `Device fault · ${set.error}`} detail={set.error} />;
}

function Refused({ set }: { set: DeviceSet }) {
  const said = refusalSaid(set);
  return said == null ? null : <FaceFault message={said} detail={set.refused?.error} />;
}

export function DeviceFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const queryClient = useQueryClient();
  const attached = useQuery(devicesQuery());
  const reference = node.kind === "device" ? (node.data.device ?? null) : null;
  const lockedStreams = node.kind === "device" ? (node.data.locked_streams ?? []) : [];
  const split = node.kind === "device" && node.data.split_tuning === true;
  const set = workspace.devices.get(node.id) ?? null;
  const onBus =
    reference !== null &&
    (attached.data?.devices ?? []).some((device) => refMatches(reference, device));

  const editNode = (next: Partial<DeviceNodeData>): void =>
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (stored) =>
        stored.kind === "device" ? { ...stored, data: { ...stored.data, ...next } } : stored,
      ),
    }));

  const nameRadio = (chosen: DeviceRef | null): void => editNode({ device: chosen });

  const forget = (): void => nameRadio(null);

  const bind = (device: DeviceInfo): void => {
    nameRadio(deviceRefOf(device));
    workspace.apply();
  };

  const openNetwork = useMutation({
    mutationFn: addNetworkRadio,
    onSuccess: bind,
    onError: (error: Error) => toastError(error),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: STATE_KEY }),
  });

  if (reference === null) {
    return (
      <NodeShell node={node} title="Device" category="source">
        <FaceBody>
          <div className="flex flex-col gap-2 p-2">
            <DeviceChoices
              onChoose={bind}
              onAddNetwork={(id) => openNetwork.mutate(id)}
              busy={openNetwork.isPending}
              error={openNetwork.error?.message ?? null}
              claimed={claimedDevices(workspace.graph, node.id)}
            />
          </div>
        </FaceBody>
      </NodeShell>
    );
  }

  if (set === null) {
    return (
      <NodeShell
        node={node}
        title="Device"
        category="source"
        subtitle={onBus ? "not open" : "disconnected"}
      >
        <FaceBody>
          <p className="p-3 font-mono text-sm text-ink">{refLabel(reference)}</p>
        </FaceBody>
        <FaceFooter>
          <Button
            type="button"
            className={BTN_QUIET_DANGER}
            title="Free this node so you can pick a different radio"
            onClick={forget}
          >
            Forget radio
          </Button>
          <Button
            type="button"
            className={BTN_PRIMARY}
            title={
              onBus
                ? "Open this radio and start the channels wired to it"
                : "Nothing to open until the radio is plugged back in"
            }
            onClick={() => workspace.apply()}
            disabled={!onBus}
          >
            Open radio
          </Button>
        </FaceFooter>
      </NodeShell>
    );
  }

  const holds = laneHolds(workspace.graph, workspace.resolved, node.id);
  const heldBy = new Map([...holds].map(([stream, hold]) => [stream, hold.label]));
  const merged = lanesMerged(set);
  const dials = tunerDials(set);
  const streams = rxStreamCount(set.capabilities);
  const rowPerPort = laneLayout(set.capabilities).lanes === streams;

  return (
    <NodeShell
      node={node}
      title={set.device.label}
      category="source"
      subtitle={<Heard set={set} />}
    >
      <FaceBody>
        <RadioSettings
          node={node.id}
          active={set}
          className="p-2"
          advised={new Set(holds.keys())}
          heldBy={heldBy}
          lead={
            <Tuner
              node={node.id}
              set={set}
              holds={holds}
              lockedStreams={lockedStreams}
              onLock={(locked_streams) => editNode({ locked_streams })}
              split={split}
              onSplit={(split_tuning) => editNode({ split_tuning })}
            />
          }
          laneLeads={
            merged && split
              ? dials.map((dial) => (
                  <LaneDial
                    key={dial.stream}
                    node={node.id}
                    set={set}
                    dial={dial}
                    locked={lockedStreams.includes(dial.stream)}
                    hold={dialHold(holds, dial.stream, true)}
                  />
                ))
              : undefined
          }
          ports={
            rowPerPort
              ? Array.from({ length: streams }, (_, stream) => streamPort("iq", stream))
              : undefined
          }
        />

        {set.error != null && <Fault set={set} />}
        <Refused set={set} />
      </FaceBody>
      <FaceFooter>
        <DeviceHealth set={set} />
        {offersMakeArray(workspace.resolved, node.id, set) && (
          <MakeArrayButton node={node.id} set={set} />
        )}
        <Button
          type="button"
          className={BTN_QUIET_DANGER}
          title="Close this radio and free the node: the device is released and the wires stay drawn"
          onClick={forget}
        >
          Forget radio
        </Button>
      </FaceFooter>
    </NodeShell>
  );
}

const LOSS_HINT =
  "The radio sends more than its link or this computer carries. Lower the rate or the lanes";

function DeviceHealth({ set }: { set: DeviceSet }) {
  const clipping = clippingSaid(set);
  const loss = lossSaid(set);
  const bond = lanesMerged(set) ? bondSaid(set.capabilities.coherence) : null;
  return (
    <SourceHealth set={set}>
      {loss !== null && (
        <Stat label="Lost" title={LOSS_HINT} tone="warn">
          {loss}
        </Stat>
      )}
      {clipping !== null && (
        <Stat label="Clipping" title="The ADC is at full scale. Lower the gain" tone="danger">
          {clipping}
        </Stat>
      )}
      {bond !== null && (
        <Stat
          label={<Icon glyph={Link2} size={12} />}
          title="The lanes sample on one clock, so their streams line up in time"
        >
          {bond}
        </Stat>
      )}
    </SourceHealth>
  );
}
