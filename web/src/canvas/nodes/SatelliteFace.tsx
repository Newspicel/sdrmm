import { useQuery } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { Button, Input } from "../../components/BaseControls";
import { BTN, BTN_QUIET, FACE_BTN, FIELD } from "../../components/controls";
import { dialId, FrequencyDial } from "../../components/FrequencyDial";
import { ChipField, Chips, SettingChip } from "../../components/face/Chips";
import { FaceFault } from "../../components/face/Fault";
import { Readout, Readouts } from "../../components/face/Readouts";
import { FaceStats, Stat } from "../../components/face/Stats";
import { formatMhz } from "../../components/format";
import { Icon } from "../../components/Icon";
import { SearchableSelect } from "../../components/SearchableSelect";
import { TuneTo } from "../../components/TuneTo";
import { TuningLock } from "../../components/TuningLock";
import { satellitesQuery, transmittersQuery } from "../../lib/api";
import { useSatelliteStore } from "../../lib/satellite";
import type {
  CatalogSatellite,
  PassPoint,
  PatchNode,
  PatchNodeOf,
  SatelliteNode,
  SatelliteStatus,
} from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { Inspect, InspectSection } from "../inspector";
import { FaceBody, FaceEmpty, FaceFooter, NodeShell, useFaceActive } from "./NodeShell";
import { useUnwired, type Want } from "./needs";
import { SkyDome } from "./SkyDome";
import {
  compass,
  formatDoppler,
  passLine,
  passWhen,
  pastedElements,
  SATELLITE_RANGE,
  STALE_ELEMENTS_DAYS,
  shownSignals,
  sourceLabel,
  transmitterLabel,
  useSecondsNow,
} from "./satellite";

const SEARCH_DELAY_MS = 350;

export function SatelliteFace({ node }: { node: PatchNode }) {
  if (node.kind !== "satellite") {
    return null;
  }
  return <SatelliteNodeFace node={node} />;
}

function SatelliteNodeFace({ node }: { node: PatchNodeOf<"satellite"> }) {
  const workspace = useWorkspaceContext();
  const status = useSatelliteStore((store) => store.byNode[node.id]) ?? null;
  const unwired = useUnwired(node.id, WANTS_POSITION);
  const edit = (next: Partial<SatelliteNode>): void =>
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        current.kind === "satellite" ? { ...current, data: { ...current.data, ...next } } : current,
      ),
    }));
  const data = node.data;
  if (data.tle == null) {
    return (
      <NodeShell node={node} title="Satellite" category="tool">
        <Inspect node={node.id}>
          <InspectSection label="Satellite">
            <SatellitePicker onPick={(tle) => edit({ tle })} />
          </InspectSection>
        </Inspect>
        <FaceBody>
          <FaceEmpty hint="No satellite" />
        </FaceBody>
      </NodeShell>
    );
  }
  return (
    <NodeShell
      node={node}
      title="Satellite"
      category="tool"
      subtitle={status?.name ?? status?.catalog ?? undefined}
    >
      <Inspect node={node.id}>
        <InspectSection label="Satellite">
          <SignalChip catalog={status?.catalog ?? null} data={data} onEdit={edit} />
        </InspectSection>
        <InspectSection label="Track">
          <TrackDetail status={status} />
        </InspectSection>
      </Inspect>
      <FaceBody>
        <Downlink
          node={node.id}
          hz={data.downlink_hz ?? null}
          held={data.transmitter != null}
          locked={data.tuning_locked ?? false}
          onLock={(tuning_locked) => edit({ tuning_locked })}
          onTune={(downlink_hz) => edit({ downlink_hz, uplink_hz: null, transmitter: null })}
        />
        <Track status={status} standing={unwired ? null : (status?.waiting ?? null)} />
        {!unwired && status?.error != null && <FaceFault message={status.error} />}
      </FaceBody>
      <FaceFooter>
        <FaceStats>
          <ElementsAge status={status} />
          <RefreshElements catalog={status?.catalog ?? null} onFresh={(tle) => edit({ tle })} />
        </FaceStats>
        <Button
          type="button"
          className={FACE_BTN}
          title="Pick another satellite"
          onClick={() => edit({ tle: null, downlink_hz: null, uplink_hz: null, transmitter: null })}
        >
          Change
        </Button>
      </FaceFooter>
    </NodeShell>
  );
}

const WANTS_POSITION: readonly Want[] = [{ port: "position" }];

function useDebounced(value: string): string {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setSettled(value), SEARCH_DELAY_MS);
    return () => clearTimeout(timer);
  }, [value]);
  return settled;
}

function SatellitePicker({ onPick }: { onPick: (tle: string) => void }) {
  const [draft, setDraft] = useState("");
  const now = useSecondsNow();
  const pasted = pastedElements(draft);
  const search = useDebounced(pasted === null ? draft.trim() : "");
  const results = useQuery({ ...satellitesQuery(search), enabled: pasted === null });
  const found: readonly CatalogSatellite[] = results.data?.satellites ?? [];
  return (
    <div className="flex flex-col gap-2">
      <Input
        aria-label="Search satellites"
        title="A name, a NORAD number, or pasted element lines"
        placeholder="ISS, 25544 or element lines"
        className={`${FIELD} w-full`}
        value={draft}
        onChange={(event) => setDraft(event.currentTarget.value)}
      />
      {pasted !== null ? (
        <Button type="button" className={BTN} onClick={() => onPick(pasted)}>
          Use these elements
        </Button>
      ) : results.isError ? (
        <p role="alert" className="text-xs text-danger">
          {results.error.message}
        </p>
      ) : (
        <ul
          aria-label="Satellites"
          className="flex max-h-72 flex-col overflow-y-auto rounded-control border border-line bg-well py-1"
          title={
            results.data === undefined
              ? undefined
              : `From ${sourceLabel(results.data.source, results.data.fetched ?? 0, now)}`
          }
        >
          {found.map((satellite) => (
            <li key={satellite.catalog}>
              <Button
                type="button"
                className={`${BTN_QUIET} w-full shrink-0 justify-between rounded-none`}
                onClick={() => onPick(satellite.tle)}
              >
                <span className="truncate">{satellite.name}</span>
                <span className="legend">{satellite.catalog}</span>
              </Button>
            </li>
          ))}
          {results.isSuccess && found.length === 0 && (
            <li className="px-2 py-1 text-xs text-ink-dim">Nothing found</li>
          )}
        </ul>
      )}
    </div>
  );
}

function Downlink({
  node,
  hz,
  held,
  locked,
  onLock,
  onTune,
}: {
  node: string;
  hz: number | null;
  held: boolean;
  locked: boolean;
  onLock: (locked: boolean) => void;
  onTune: (hz: number) => void;
}) {
  const active = useFaceActive();
  return (
    <div className="flex min-w-0 items-center gap-1 p-2" title="Downlink before Doppler">
      <div className="@container min-w-0 flex-1">
        <FrequencyDial
          id={dialId(node)}
          hz={hz ?? 0}
          range={SATELLITE_RANGE}
          disabled={held || locked}
          wheelTunes={active}
          onTune={onTune}
        />
      </div>
      <span className="ml-auto flex shrink-0 items-center gap-1">
        <TuneTo
          title="Type the downlink"
          hz={hz ?? 0}
          hint="Before Doppler"
          resolve={(entered) =>
            entered > SATELLITE_RANGE.min && entered <= SATELLITE_RANGE.max ? entered : null
          }
          disabled={held || locked}
          onTune={onTune}
        />
        <TuningLock
          locked={held || locked}
          held="Downlink locked"
          free="Lock downlink"
          hold={held ? "Set by the signal. Pick Own frequency to tune by hand." : null}
          onLock={onLock}
        />
      </span>
    </div>
  );
}

function SignalChip({
  catalog,
  data,
  onEdit,
}: {
  catalog: string | null;
  data: SatelliteNode;
  onEdit: (next: Partial<SatelliteNode>) => void;
}) {
  const listed = useQuery(transmittersQuery(catalog));
  const now = useSecondsNow();
  const transmitters = shownSignals(listed.data?.transmitters ?? [], data.transmitter);
  if (transmitters.length === 0) {
    return null;
  }
  const picked = transmitters.find((transmitter) => transmitter.id === data.transmitter);
  return (
    <Chips inset="none">
      <SettingChip
        label="Signal"
        value={picked === undefined ? "own" : transmitterLabel(picked)}
        quiet={picked === undefined}
        disabled={data.tuning_locked ?? false}
        title={`What this satellite sends, from ${sourceLabel(listed.data?.source ?? "SatNOGS DB", listed.data?.fetched ?? 0, now)}`}
      >
        {() => (
          <ChipField label="Signal">
            <SearchableSelect
              label="Signal"
              className="min-w-0 flex-1"
              value={data.transmitter ?? ""}
              options={[
                { value: "", label: "Own frequency" },
                ...transmitters.map((transmitter) => ({
                  value: transmitter.id,
                  label: transmitterLabel(transmitter),
                })),
              ]}
              onChange={(id) => {
                const transmitter = transmitters.find((candidate) => candidate.id === id);
                onEdit(
                  transmitter === undefined
                    ? { transmitter: null }
                    : {
                        transmitter: transmitter.id,
                        downlink_hz: transmitter.downlink_hz ?? null,
                        uplink_hz: transmitter.uplink_hz ?? null,
                      },
                );
              }}
            />
          </ChipField>
        )}
      </SettingChip>
    </Chips>
  );
}

/**
 * Where the satellite is: the sky from the ground station beside what tracking needs at a
 * glance. Until it can say, the dome carries the same faint hint every plot uses and the
 * readouts keep their rows.
 */
function Track({ status, standing }: { status: SatelliteStatus | null; standing: string | null }) {
  const now = useSecondsNow();
  const look = standing === null ? (status?.look ?? null) : null;
  const doppler = standing === null ? (status?.doppler_hz ?? null) : null;
  const pass = standing === null && look !== null ? (status?.next_pass ?? null) : null;
  return (
    <div className="flex items-center gap-3 border-t border-line p-2">
      <div className="relative shrink-0">
        <SkyDome
          look={look}
          track={standing === null ? (status?.pass_track ?? NO_TRACK) : NO_TRACK}
          visible={status?.visible === true}
          quiet={standing !== null}
        />
        {standing !== null && (
          <span className="absolute inset-0 flex items-center justify-center p-6 text-center text-2xs text-balance text-ink-faint">
            {standing}
          </span>
        )}
      </div>
      <dl className="grid min-w-0 flex-1 grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 text-xs">
        <TrackValue label="Elevation" tone={status?.visible === true ? "text-ok" : undefined}>
          {look === null ? null : `${look.elevation_deg.toFixed(1)}°`}
        </TrackValue>
        <TrackValue label="Azimuth">
          {look === null ? null : `${look.azimuth_deg.toFixed(0)}° ${compass(look.azimuth_deg)}`}
        </TrackValue>
        <TrackValue label="Doppler" title="Added to every wired decoder">
          {doppler === null ? null : formatDoppler(doppler)}
        </TrackValue>
        <TrackValue label="Next pass" title={pass === null ? undefined : passLine(pass, now)}>
          {look === null ? null : passWhen(pass, now)}
        </TrackValue>
      </dl>
    </div>
  );
}

/** One row beside the dome: the value at the far edge, a faint dash until there is one. */
function TrackValue({
  label,
  title,
  tone,
  children,
}: {
  label: string;
  title?: string;
  tone?: string;
  children: string | null;
}) {
  return (
    <>
      <dt className="text-ink-dim">{label}</dt>
      <dd
        title={title}
        className={`truncate text-right font-mono tabular-nums ${children === null ? "text-ink-faint" : (tone ?? "text-ink")}`}
      >
        {children ?? "-"}
      </dd>
    </>
  );
}

const NO_TRACK: readonly PassPoint[] = [];

/** What the face leaves out: how far away it is, how fast the shift moves, where to send. */
function TrackDetail({ status }: { status: SatelliteStatus | null }) {
  const look = status?.look ?? null;
  const rate = status?.doppler_rate_hz_s ?? null;
  const uplink = status?.uplink_hz ?? null;
  return (
    <Readouts>
      <Readout label="Range">{look === null ? "-" : `${look.range_km.toFixed(0)} km`}</Readout>
      <Readout label="Doppler rate">{rate === null ? "-" : `${formatDoppler(rate)}/s`}</Readout>
      <Readout label="Send on" title="The uplink corrected so the satellite hears it on frequency">
        {uplink === null ? "-" : formatMhz(uplink)}
      </Readout>
    </Readouts>
  );
}

function ElementsAge({ status }: { status: SatelliteStatus | null }) {
  const days = status?.tle_age_days;
  if (days == null) {
    return null;
  }
  return (
    <Stat
      label="Elements"
      title="Age of the orbital elements. Older ones drift; refresh them"
      tone={days > STALE_ELEMENTS_DAYS ? "warn" : undefined}
    >
      {days.toFixed(1)} d
    </Stat>
  );
}

function RefreshElements({
  catalog,
  onFresh,
}: {
  catalog: string | null;
  onFresh: (tle: string) => void;
}) {
  const fresh = useQuery({ ...satellitesQuery(catalog ?? ""), enabled: false });
  if (catalog === null) {
    return null;
  }
  return (
    <Button
      type="button"
      className={FACE_BTN}
      disabled={fresh.isFetching}
      aria-label="Refresh elements"
      title={fresh.isError ? fresh.error.message : "Fetch the newest elements"}
      onClick={() =>
        void fresh.refetch().then((result) => {
          const match = result.data?.satellites.find((satellite) => satellite.catalog === catalog);
          if (match !== undefined) {
            onFresh(match.tle);
          }
        })
      }
    >
      <Icon glyph={RefreshCw} />
    </Button>
  );
}
