import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { nodeAction, STATE_KEY } from "../../lib/api";
import { channelNodesOn, feedsOf } from "../../lib/resolved";
import { toastError } from "../../lib/toasts";
import type { ChannelInfo, ChannelParams, DeviceSet } from "../../lib/types";
import { useChannelPatch } from "../../lib/useChannelPatch";
import { useNodeTune } from "../../lib/useNodeTune";
import { useWorkspaceContext } from "../context";
import { addEdge, addNode, newNodeId, nodeIds, nodeOf } from "../graph";
import type { IqLane } from "../lanes";
import { useNodePlacement } from "../placement";
import { lockedChannels } from "./channelNode";
import { autoTuning, laneLocked } from "./deviceNode";
import { type TrunkChannelOwner, trunkChannelRoles } from "./dmrTrunk";
import type { FrameMeta } from "./scopeFeed";
import { type ScopePick, takeCreationTune, tuneOnCreate } from "./scopePick";

const NO_OWNERS: ReadonlyMap<number, TrunkChannelOwner> = new Map();

export interface ScopeChannels {
  faces: ReadonlyMap<number, string>;
  owners: ReadonlyMap<number, TrunkChannelOwner>;
  locked: ReadonlyMap<number, string>;
  heldChannel: (channel: number) => boolean;
  selectedChannel: number | null;
  tunableChannel: number | null;
  centerHeld: boolean;
  selectChannel: (channel: number) => void;
  tuneChannel: (channel: number, frequencyHz: number) => void;
  tuneCenter: (hz: number) => void;
  tuneTo: (pick: ScopePick) => void;
  onBandTune: (hz: number, suggested: ChannelParams | null) => void;
  addChannelAt: (pick: ScopePick, channelType: string) => void;
}

function useLaneTune(node: string, set: DeviceSet | null, source: IqLane | null) {
  const tuneNode = useNodeTune();
  const { graph } = useWorkspaceContext();
  const stream = source?.stream ?? 0;
  const locked = source !== null && laneLocked(nodeOf(graph, source.source), stream);
  const centerHeld =
    source?.virtual === undefined && set !== null && (autoTuning(set, stream) || locked);
  return {
    centerHeld,
    tuneCenter: (hz: number): void => {
      if (!centerHeld) {
        tuneNode(node, hz);
      }
    },
  };
}

function facesOn(
  resolved: ReturnType<typeof useWorkspaceContext>["resolved"],
  setId: number | null,
  channels: readonly ChannelInfo[],
  owners: ReadonlyMap<number, TrunkChannelOwner>,
): Map<number, string> {
  const faces = new Map<number, string>();
  const onStream = new Set(channels.map((channel) => channel.id));
  if (setId !== null) {
    for (const [channel, face] of channelNodesOn(resolved, setId)) {
      if (onStream.has(channel)) {
        faces.set(channel, face);
      }
    }
  }
  for (const [channel, owner] of owners) {
    if (onStream.has(channel)) {
      faces.set(channel, owner.node);
    }
  }
  return faces;
}

export function useScopeChannels(input: {
  node: string;
  set: DeviceSet | null;
  source: IqLane | null;
  channels: readonly ChannelInfo[];
  meta: FrameMeta | null;
}): ScopeChannels {
  const { node, set, source, channels, meta } = input;
  const workspace = useWorkspaceContext();
  const setId = set?.id ?? null;
  const { applyEdit } = useChannelPatch();
  const tuneNode = useNodeTune();
  const queryClient = useQueryClient();
  const placeNode = useNodePlacement();
  const [picked, setPicked] = useState<number | null>(null);

  const owners = setId === null ? NO_OWNERS : trunkChannelRoles(workspace.trunks, setId);
  const faces = facesOn(workspace.resolved, setId, channels, owners);
  const locked = lockedChannels(workspace.resolved, faces);
  const heldChannel = (channel: number): boolean => owners.has(channel) || locked.has(channel);
  const { centerHeld, tuneCenter } = useLaneTune(node, set, source);

  const workspaceChannel = [...faces].find(([, id]) => id === workspace.selected)?.[0] ?? null;
  const selectedChannel =
    workspaceChannel ?? (channels.some((channel) => channel.id === picked) ? picked : null);
  const tunableChannel =
    selectedChannel !== null && !heldChannel(selectedChannel) ? selectedChannel : null;

  const tuneChannel = (channel: number, frequencyHz: number): void => {
    const face = faces.get(channel);
    if (face !== undefined && !heldChannel(channel)) {
      tuneNode(face, frequencyHz);
    }
  };

  const tuneToBand = (hz: number, suggested: ChannelParams | null): void => {
    const face = tunableChannel === null ? undefined : faces.get(tunableChannel);
    if (face === undefined) {
      tuneCenter(hz);
      return;
    }
    if (meta === null || Math.abs(hz - meta.centerHz) >= meta.spanHz / 2) {
      tuneCenter(hz);
    }
    if (suggested === null) {
      applyEdit(face, { frequency_hz: Math.round(hz) });
      return;
    }
    void nodeAction(face, { action: "retype", with: { channel_type: suggested.type } })
      .then(() => queryClient.refetchQueries({ queryKey: STATE_KEY }))
      .then(() => applyEdit(face, { frequency_hz: Math.round(hz), params: suggested }))
      .catch(toastError);
  };

  const editRef = useRef(applyEdit);
  const bandTuneRef = useRef(tuneToBand);
  useLayoutEffect(() => {
    editRef.current = applyEdit;
    bandTuneRef.current = tuneToBand;
  });
  const onBandTune = useCallback(
    (hz: number, suggested: ChannelParams | null) => bandTuneRef.current(hz, suggested),
    [],
  );

  useEffect(() => {
    if (setId === null) {
      return;
    }
    for (const face of faces.values()) {
      const frequencyHz = takeCreationTune(face);
      if (frequencyHz !== undefined) {
        editRef.current(face, { frequency_hz: frequencyHz });
      }
    }
  });

  return {
    faces,
    owners,
    locked,
    heldChannel,
    selectedChannel,
    tunableChannel,
    centerHeld,
    selectChannel: (channel) => {
      setPicked(channel);
      const face = faces.get(channel);
      if (face !== undefined) {
        workspace.select(face);
      }
    },
    tuneChannel,
    tuneCenter,
    tuneTo: (pick) => {
      if (tunableChannel !== null) {
        tuneChannel(tunableChannel, pick.hz);
      } else {
        tuneCenter(pick.hz);
      }
    },
    onBandTune,
    addChannelAt: (pick, channelType) => {
      const feed = feedsOf(workspace.resolved, node, "iq")[0];
      if (feed === undefined) {
        return;
      }
      const fed = { node: feed.source, port: feed.port };
      const id = newNodeId("channel", nodeIds(workspace.graph));
      tuneOnCreate(id, pick.hz);
      const added = {
        id,
        kind: "channel" as const,
        data: { channel_type: channelType },
        position: placeNode(workspace.graph, "channel"),
      };
      workspace.edit((snapshot) => ({
        ...snapshot,
        graph: addEdge(addNode(snapshot.graph, added), {
          from: fed,
          to: { node: id, port: "iq" },
        }),
      }));
      workspace.apply();
      workspace.select(id);
    },
  };
}
