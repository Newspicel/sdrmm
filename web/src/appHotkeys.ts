import { isPinned, pin, unpin } from "./canvas/graph";
import { nextAnalogMode, retypeNode } from "./canvas/nodes/decoderSwap";
import { type HotkeyActions, useHotkeys } from "./canvas/useHotkeys";
import type { WorkspaceStore } from "./canvas/useWorkspace";
import type { View } from "./canvas/WorkspaceBar";
import { DEFAULT_SQUELCH_DB, nudgedSquelch, SQUELCH_OFF } from "./components/channelSettings";
import { TUNE_STEPS_HZ } from "./components/dial";
import { dialId } from "./components/FrequencyDial";
import { pushToast, toastError } from "./lib/toasts";
import type { ChannelInfo, ChannelSettings, DeviceSet, PatchGraph, PatchNode } from "./lib/types";
import type { ChannelEdit } from "./lib/useChannelPatch";
import { forStream } from "./lib/useDevicePatch";

export const PICK_A_RADIO = "Select a radio or decoder";
export const PICK_A_DECODER = "Select a decoder";

export function nodeFrequencyHz(
  selected: string | null,
  devices: ReadonlyMap<string, DeviceSet>,
  channels: ReadonlyMap<string, ChannelInfo>,
  saved: ReadonlyMap<string, ChannelSettings>,
): number | null {
  if (selected === null) {
    return null;
  }
  const set = devices.get(selected);
  if (set !== undefined) {
    return forStream(set.settings, 0, set.capabilities.per_stream).center_hz ?? null;
  }
  return (channels.get(selected)?.settings ?? saved.get(selected))?.frequency_hz ?? null;
}

export interface NodeHotkeys {
  selectedNode: PatchNode | null;
  selectedHz: number | null;
  stepHz: number;
  tuneNode: (node: string, hz: number) => void;
  applyEdit: (node: string, edit: ChannelEdit) => void;
  retype?: typeof retypeNode;
  notify?: (message: string) => void;
}

export function nodeHotkeys(
  b: NodeHotkeys,
): Pick<HotkeyActions, "tune" | "cycleMode" | "adjustSquelch" | "toggleSquelch"> {
  const notify = b.notify ?? ((message: string) => pushToast(message, "info"));
  const decoder = (): PatchNode | null => {
    if (b.selectedNode?.kind === "channel") {
      return b.selectedNode;
    }
    notify(PICK_A_DECODER);
    return null;
  };
  return {
    tune: (steps) => {
      if (b.selectedNode === null || b.selectedHz === null) {
        notify(PICK_A_RADIO);
        return;
      }
      b.tuneNode(b.selectedNode.id, b.selectedHz + steps * b.stepHz);
    },
    cycleMode: (direction) => {
      const node = decoder();
      if (node?.kind === "channel") {
        const wanted = nextAnalogMode(node.data.channel_type, direction);
        (b.retype ?? retypeNode)(node.id, wanted).catch(toastError);
      }
    },
    adjustSquelch: (deltaDb) => {
      const node = decoder();
      if (node !== null) {
        b.applyEdit(node.id, (current) => ({ squelch: nudgedSquelch(current.squelch, deltaDb) }));
      }
    },
    toggleSquelch: () => {
      const node = decoder();
      if (node !== null) {
        b.applyEdit(node.id, (current) => ({
          squelch:
            current.squelch?.mode === "off" || current.squelch === undefined
              ? { mode: "manual", level_db: DEFAULT_SQUELCH_DB }
              : SQUELCH_OFF,
        }));
      }
    },
  };
}

export interface AppHotkeys extends NodeHotkeys {
  selected: string | null;
  setSelected: (id: string | null) => void;
  channelNodes: readonly PatchNode[];
  graph: PatchGraph;
  setStepHz: (hz: number) => void;
  workspace: WorkspaceStore;
  setView: (update: (current: View) => View) => void;
  setExpanded: (update: (current: string | null) => string | null) => void;
  setShowHelp: (show: boolean) => void;
}

export function useAppHotkeys(b: AppHotkeys) {
  useHotkeys({
    ...nodeHotkeys(b),
    stepBy: (direction) => {
      const at = TUNE_STEPS_HZ.indexOf(b.stepHz as (typeof TUNE_STEPS_HZ)[number]);
      const next = Math.min(TUNE_STEPS_HZ.length - 1, Math.max(0, at + direction));
      b.setStepHz(TUNE_STEPS_HZ[next] ?? b.stepHz);
    },
    focusDial: () => {
      if (b.selected !== null) {
        document.getElementById(dialId(b.selected))?.focus();
      }
    },
    selectChannel: (direction) => {
      if (b.channelNodes.length === 0) {
        return;
      }
      const at = b.channelNodes.findIndex((node) => node.id === b.selected);
      const next = b.channelNodes[(at + direction + b.channelNodes.length) % b.channelNodes.length];
      b.setSelected(next?.id ?? null);
    },
    selectNode: (index) => b.setSelected(b.graph.nodes[index]?.id ?? null),
    togglePin: () => {
      const node = b.selectedNode;
      if (node === null) {
        return;
      }
      b.workspace.save((current) => ({
        ...current,
        rack: isPinned(current.rack ?? {}, node.id)
          ? unpin(current.rack ?? {}, node.id)
          : pin(current.rack ?? {}, node.id),
      }));
    },
    toggleView: () => b.setView((current) => (current === "patch" ? "rack" : "patch")),
    toggleFull: () => {
      const node = b.selectedNode;
      b.setExpanded((current) => (current !== null || node === null ? null : node.id));
    },
    undo: b.workspace.undo,
    redo: b.workspace.redo,
    showHelp: () => b.setShowHelp(true),
  });
}
