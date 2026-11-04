import { useMutation, useQuery } from "@tanstack/react-query";
import { useCallback } from "react";
import { create } from "zustand";
import { fusionQuery, resetFusion } from "./api";
import { omitNodes } from "./byNode";
import { clearAction, failAction } from "./refusals";
import type { ChannelStatus, DfFusionState, DfFusionView, ServerEvent } from "./types";
import { useSeed } from "./useSeed";

export interface FusionStore {
  byNode: Readonly<Record<string, DfFusionState>>;
  statusByNode: Readonly<Record<string, ChannelStatus>>;
  observe: (event: ServerEvent) => void;
  set: (node: string, state: DfFusionState, status?: ChannelStatus) => void;
  forget: (nodes: readonly string[]) => void;
  reset: () => void;
}

export const useFusionStore = create<FusionStore>((set) => ({
  byNode: {},
  statusByNode: {},
  observe: (event) => {
    if (event.type === "Hello") {
      set({ byNode: {}, statusByNode: {} });
    } else if (event.type === "DfFusionUpdate") {
      const { node, state: fusion, status } = event.data;
      set((state) => ({
        byNode: { ...state.byNode, [node]: fusion },
        statusByNode:
          status === undefined ? state.statusByNode : { ...state.statusByNode, [node]: status },
      }));
    }
  },
  set: (node, fusion, status) =>
    set((state) => ({
      byNode: { ...state.byNode, [node]: fusion },
      statusByNode:
        status === undefined ? state.statusByNode : { ...state.statusByNode, [node]: status },
    })),
  forget: (nodes) =>
    set((state) => ({
      byNode: omitNodes(state.byNode, nodes),
      statusByNode: omitNodes(state.statusByNode, nodes),
    })),
  reset: () => set({ byNode: {}, statusByNode: {} }),
}));

export const FUSION_ACTION = "Fusion";
export const CLEAR_ACTION = "Clear";

export function useFusionSeed(node: string): void {
  const query = useQuery(fusionQuery(node));
  const held = useFusionStore((store) => store.byNode[node] !== undefined);
  const apply = useCallback(
    (view: DfFusionView) => useFusionStore.getState().set(node, view.state, view.status),
    [node],
  );
  useSeed(node, FUSION_ACTION, query, held, apply);
}

export function useFusionClear(node: string): { clear: () => void; pending: boolean } {
  const reset = useMutation({
    mutationFn: () => resetFusion(node),
    onSuccess: () => clearAction(node, CLEAR_ACTION),
    onError: (error) => failAction(node, CLEAR_ACTION, error),
  });
  return { clear: () => reset.mutate(), pending: reset.isPending };
}
