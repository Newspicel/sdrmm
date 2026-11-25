import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Button, Form, Input } from "../../components/BaseControls";
import { BTN, BTN_QUIET_DANGER, BTN_ROW, FIELD } from "../../components/controls";
import {
  ChipField,
  Chips,
  ChoiceChip,
  NumberChip,
  ReadoutChip,
  SettingChip,
} from "../../components/face/Chips";
import { FaceFault } from "../../components/face/Fault";
import { Readout, Readouts } from "../../components/face/Readouts";
import { TextChip } from "../../components/face/TextChip";
import { Segmented } from "../../components/Segmented";
import { TextAutocomplete } from "../../components/TextAutocomplete";
import { serialPortsQuery } from "../../lib/api";
import { ROTATOR_LIMITS } from "../../lib/limits";
import { useRotatorStore } from "../../lib/rotator";
import type {
  PatchNode,
  PatchNodeOf,
  RotatorAxes,
  RotatorLink,
  RotatorNode,
  RotatorStatus,
  SkyPoint,
} from "../../lib/types";
import { useWorkspaceNow, useWorkspaceSelect } from "../context";
import { patchNode } from "../graph";
import { Inspect, InspectSection } from "../inspector";
import { nmeaSuggestion, serialDetail, validHostPort } from "./gpsSource";
import { FaceBody, FaceStanding, NodeShell } from "./NodeShell";
import { NO_WANTS, useUnwired, type Want } from "./needs";
import {
  AZIMUTH_RANGES,
  aimLabel,
  elevationMax,
  flipped,
  flipsOver,
  leadLabel,
  parsedBaud,
  ROTATOR_AXES,
  ROTATOR_BAUDS,
  ROTATOR_MODES,
  rotctldLink,
  serialLink,
  VIRTUAL_LINK,
  winegardLink,
  withRange,
} from "./rotator";
import { passWhen, useSecondsNow } from "./satellite";

type Pick = (link: RotatorLink) => void;
type LinkTab = RotatorLink["type"];
type SerialRotator = Extract<RotatorLink, { type: "gs232" | "winegard" }>;

const LINK_LABELS: Record<LinkTab, { label: string; title: string }> = {
  gs232: { label: "GS-232", title: "A Yaesu GS-232 controller on a serial port" },
  winegard: {
    label: "Winegard",
    title: "A Winegard Trav'ler motor console on a serial port",
  },
  rotctld: { label: "rotctld", title: "A Hamlib rotctld daemon" },
  virtual: { label: "Virtual", title: "A simulated mount, no hardware" },
};

const LINK_TABS = (Object.keys(LINK_LABELS) as LinkTab[]).map((value) => ({
  value,
  ...LINK_LABELS[value],
}));

const PORT_TITLES: Record<SerialRotator["type"], string> = {
  gs232: "Serial port of the GS-232 controller",
  winegard: "Serial port of the Trav'ler motor console",
};

export function RotatorFace({ node }: { node: PatchNode }) {
  if (node.kind !== "rotator") {
    return null;
  }
  return <RotatorNodeFace node={node} />;
}

function RotatorNodeFace({ node }: { node: PatchNodeOf<"rotator"> }) {
  const now = useWorkspaceNow();
  const status = useRotatorStore((store) => store.byNode[node.id]) ?? null;
  const edit = (next: Partial<RotatorNode>): void =>
    now().edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        current.kind === "rotator" ? { ...current, data: { ...current.data, ...next } } : current,
      ),
    }));
  const data = node.data;
  const link = data.link ?? null;
  const unwired = useUnwired(node.id, (data.mode ?? "track") === "track" ? WANTS_TARGET : NO_WANTS);
  if (link === null) {
    return (
      <NodeShell node={node} title="Rotator" category="output" subtitle="no link">
        <Inspect node={node.id}>
          <InspectSection label="Link">
            <LinkChoices onPick={(picked) => edit({ link: picked })} />
          </InspectSection>
        </Inspect>
        <FaceBody>
          <RotatorReadouts status={null} axes={data.axes ?? "azimuth_elevation"} />
        </FaceBody>
      </NodeShell>
    );
  }
  const axes = data.axes ?? "azimuth_elevation";
  const mode = data.mode ?? "track";
  return (
    <NodeShell node={node} title="Rotator" category="output" subtitle={trackLine(status)}>
      <Inspect node={node.id}>
        <InspectSection label="Mode">
          <Segmented
            label="Rotator mode"
            value={mode}
            options={ROTATOR_MODES}
            onChange={(next) => edit({ mode: next })}
            fill
          />
          {(mode === "manual" || mode === "park") && (
            <Chips inset="none">
              <AimChips data={data} mode={mode} onEdit={edit} />
            </Chips>
          )}
        </InspectSection>
        <InspectSection label="Link">
          <Chips inset="none">
            <LinkChips link={link} onChange={(next) => edit({ link: next })} />
          </Chips>
          <Button
            type="button"
            className={`${BTN_QUIET_DANGER} self-start`}
            onClick={() => edit({ link: null })}
          >
            Forget link
          </Button>
        </InspectSection>
        <InspectSection label="Mount">
          <Chips inset="none">
            <MountChips data={data} onEdit={edit} />
          </Chips>
        </InspectSection>
      </Inspect>
      <FaceBody>
        <RotatorReadouts status={status} axes={axes} />
        <FaceStanding unwired={unwired} error={status?.error} waiting={status?.waiting} />
      </FaceBody>
    </NodeShell>
  );
}

const WANTS_TARGET: readonly Want[] = [{ port: "control" }];

function RotatorReadouts({ status, axes }: { status: RotatorStatus | null; axes: RotatorAxes }) {
  return (
    <Readouts>
      <FollowReadouts status={status} />
      <Readout label="Pointing">{aimLabel(status?.position, axes)}</Readout>
      <Readout label="Target" title="Where the satellite is, or where it rises next">
        {aimLabel(status?.target, axes)}
      </Readout>
    </Readouts>
  );
}

function FollowReadouts({ status }: { status: RotatorStatus | null }) {
  const lead = status?.following;
  const following = useWorkspaceSelect((workspace) => leadLabel(lead, workspace.graph.nodes));
  const now = useSecondsNow();
  return (
    <>
      <Readout label="Following" title="The best pass in view, or the first to rise">
        {following ?? "-"}
      </Readout>
      <Readout label="Pass">
        {lead?.next_pass == null ? "-" : passWhen(lead.next_pass, now)}
      </Readout>
    </>
  );
}

function trackLine(status: RotatorStatus | null): string | undefined {
  if (status?.awaiting_pass === true) {
    return "waiting for rise";
  }
  return status?.flipped === true ? "over the top" : undefined;
}

type Edit = (next: Partial<RotatorNode>) => void;

function AimChips({
  data,
  mode,
  onEdit,
}: {
  data: RotatorNode;
  mode: "manual" | "park";
  onEdit: Edit;
}) {
  const aim = (mode === "manual" ? data.manual : data.park) ?? { azimuth_deg: 0, elevation_deg: 0 };
  const set = (next: Partial<SkyPoint>): void => onEdit({ [mode]: { ...aim, ...next } });
  const axes = data.axes ?? "azimuth_elevation";
  return (
    <>
      <NumberChip
        label="Az"
        title={mode === "manual" ? "Azimuth to point at" : "Park azimuth"}
        value={aim.azimuth_deg}
        unit="°"
        min={0}
        max={data.azimuth_max_deg ?? ROTATOR_LIMITS.azimuth_deg.min}
        step={1}
        onCommit={(azimuth_deg) => set({ azimuth_deg })}
      />
      {axes === "azimuth_elevation" && (
        <NumberChip
          label="El"
          title={mode === "manual" ? "Elevation to point at" : "Park elevation"}
          value={aim.elevation_deg}
          unit="°"
          min={0}
          max={elevationMax(axes, flipped(data))}
          step={1}
          onCommit={(elevation_deg) => set({ elevation_deg })}
        />
      )}
    </>
  );
}

function MountChips({ data, onEdit }: { data: RotatorNode; onEdit: Edit }) {
  const axes = data.axes ?? "azimuth_elevation";
  return (
    <>
      <ChoiceChip
        label="Axes"
        title="Azimuth and elevation, or a turntable"
        value={axes}
        options={ROTATOR_AXES}
        onChange={(next) => onEdit({ axes: next })}
      />
      <ChoiceChip
        label="Range"
        title="How far the azimuth turns from its stop"
        value={data.azimuth_max_deg ?? ROTATOR_LIMITS.azimuth_deg.min}
        options={AZIMUTH_RANGES}
        onChange={(azimuth_max_deg) => onEdit(withRange(data, azimuth_max_deg))}
      />
      <NumberChip
        label="Heading"
        title="Compass bearing the mount's 0° azimuth faces"
        value={data.heading_deg ?? 0}
        unit="°"
        min={ROTATOR_LIMITS.heading_deg.min}
        max={ROTATOR_LIMITS.heading_deg.max - 1}
        step={1}
        onCommit={(heading_deg) => onEdit({ heading_deg })}
      />
      {axes === "azimuth_elevation" && flipsOver(data.link) && (
        <ChoiceChip
          label="Flip"
          title="Elevation reaches 180°, so passes over the top need no azimuth swing"
          value={data.flip === true ? "on" : "off"}
          options={FLIP_OPTIONS}
          onChange={(next) => onEdit({ flip: next === "on" })}
        />
      )}
      <NumberChip
        label="Step"
        title="Moves smaller than this wait, to spare the motors"
        value={data.tolerance_deg ?? ROTATOR_LIMITS.tolerance_seed_deg}
        unit="°"
        min={ROTATOR_LIMITS.tolerance_deg.min}
        max={ROTATOR_LIMITS.tolerance_deg.max}
        step={0.5}
        onCommit={(tolerance_deg) => onEdit({ tolerance_deg })}
      />
    </>
  );
}

const FLIP_OPTIONS = [
  { value: "off", label: "off" },
  { value: "on", label: "on" },
];

function LinkChoices({ onPick }: { onPick: Pick }) {
  const [tab, setTab] = useState<LinkTab>("gs232");
  return (
    <div className="flex flex-col gap-2">
      <Segmented label="Rotator link" value={tab} options={LINK_TABS} onChange={setTab} fill />
      <LinkForm tab={tab} onPick={onPick} />
    </div>
  );
}

function LinkForm({ tab, onPick }: { tab: LinkTab; onPick: Pick }) {
  switch (tab) {
    case "rotctld":
      return <RotctldForm onPick={onPick} />;
    case "virtual":
      return (
        <Button type="button" className={`${BTN} self-start`} onClick={() => onPick(VIRTUAL_LINK)}>
          Connect
        </Button>
      );
    default:
      return (
        <SerialChoices onPick={onPick} make={tab === "winegard" ? winegardLink : serialLink} />
      );
  }
}

function SerialChoices({ onPick, make }: { onPick: Pick; make: (device: string) => RotatorLink }) {
  const devices = useQuery(serialPortsQuery());
  const listed = devices.data?.devices ?? [];
  return (
    <div className="flex flex-col gap-1">
      {listed.map((device) => {
        const detail = serialDetail(device);
        return (
          <Button
            key={device.path}
            type="button"
            className={BTN_ROW}
            onClick={() => onPick(make(device.path))}
          >
            <span className="flex w-full min-w-0 flex-col gap-0.5">
              <span className="truncate">{device.path}</span>
              {detail !== "" && (
                <span className="truncate font-mono text-2xs text-ink-faint">{detail}</span>
              )}
            </span>
          </Button>
        );
      })}
      {devices.isSuccess && listed.length === 0 && (
        <p className="text-ink-dim text-sm">No serial ports</p>
      )}
      {devices.isError && (
        <FaceFault message="Serial device discovery failed" detail={devices.error.message} />
      )}
    </div>
  );
}

function RotctldForm({ onPick }: { onPick: Pick }) {
  const [address, setAddress] = useState(ROTATOR_LIMITS.rotctld_address);
  const valid = validHostPort(address.trim());
  return (
    <Form
      className="flex items-center gap-2"
      onSubmit={(event) => {
        event.preventDefault();
        if (valid) {
          onPick(rotctldLink(address.trim()));
        }
      }}
    >
      <Input
        className={`${FIELD} w-full`}
        type="text"
        aria-label="rotctld address"
        title="Host and port of rotctld"
        placeholder={ROTATOR_LIMITS.rotctld_address}
        value={address}
        onChange={(event) => setAddress(event.target.value)}
      />
      <Button type="submit" className={BTN} disabled={!valid}>
        Connect
      </Button>
    </Form>
  );
}

function LinkChips({ link, onChange }: { link: RotatorLink; onChange: Pick }) {
  if (link.type === "virtual") {
    return <ReadoutChip label="Link" value="Virtual" title={LINK_LABELS.virtual.title} />;
  }
  if (link.type === "rotctld") {
    return (
      <TextChip
        label="rotctld"
        name="rotctld address"
        title="Host and port of rotctld"
        value={link.address}
        onCommit={(address) => {
          if (!validHostPort(address)) {
            return false;
          }
          onChange({ ...link, address });
          return true;
        }}
      />
    );
  }
  return <SerialChips link={link} onChange={onChange} />;
}

function SerialChips({ link, onChange }: { link: SerialRotator; onChange: Pick }) {
  const devices = useQuery(serialPortsQuery());
  return (
    <>
      <SettingChip label="Port" value={link.device} title={PORT_TITLES[link.type]}>
        {() => (
          <ChipField label="Serial device">
            <TextAutocomplete
              value={link.device}
              label="Serial device"
              className="min-w-0 flex-1"
              placeholder="Pick one or type a path"
              suggestions={(devices.data?.devices ?? []).map(nmeaSuggestion)}
              onCommit={(device) => {
                if (device === "") {
                  return false;
                }
                onChange({ ...link, device });
                return true;
              }}
            />
          </ChipField>
        )}
      </SettingChip>
      <SettingChip label="Baud" value={String(link.baud)} title="Serial baud rate" width="w-52">
        {() => (
          <ChipField label="Baud">
            <TextAutocomplete
              value={String(link.baud)}
              label="Baud"
              className="min-w-0 flex-1"
              inputMode="numeric"
              suggestions={ROTATOR_BAUDS}
              onCommit={(value) => {
                const baud = parsedBaud(value);
                if (baud !== null) {
                  onChange({ ...link, baud });
                }
                return baud !== null;
              }}
            />
          </ChipField>
        )}
      </SettingChip>
    </>
  );
}
