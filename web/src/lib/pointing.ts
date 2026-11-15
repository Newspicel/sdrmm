import { useMutation } from "@tanstack/react-query";
import { create } from "zustand";
import { controlSkyScan } from "./api";
import { clearAction, failAction } from "./refusals";
import type { AimStatus, ServerEvent, SkyScanStatus, SurveyAction } from "./types";

interface StatusStore<T> {
  byNode: Readonly<Record<string, T>>;
  observe: (event: ServerEvent) => void;
  put: (status: T) => void;
  reset: () => void;
}

function statusStore<T extends { node: string }>(pick: (event: ServerEvent) => T | null) {
  return create<StatusStore<T>>((set) => {
    const put = (status: T): void =>
      set((state) => ({ byNode: { ...state.byNode, [status.node]: status } }));
    return {
      byNode: {},
      observe: (event) => {
        const status = pick(event);
        if (status !== null) {
          put(status);
        }
      },
      put,
      reset: () => set({ byNode: {} }),
    };
  });
}

export const useAimStore = statusStore<AimStatus>((event) =>
  event.type === "AimUpdate" ? event.data.status : null,
);

export const useSkyScanStore = statusStore<SkyScanStatus>((event) =>
  event.type === "SkyScanUpdate" ? event.data.status : null,
);

export const SKY_SCAN_ACTION = "Sky scan";

export function useSkyScanControl(node: string): {
  control: (action: SurveyAction) => void;
  pending: boolean;
} {
  const send = useMutation({
    mutationFn: (action: SurveyAction) => controlSkyScan(node, action),
    onSuccess: (status) => {
      useSkyScanStore.getState().put(status);
      clearAction(node, SKY_SCAN_ACTION);
    },
    onError: (error) => failAction(node, SKY_SCAN_ACTION, error),
  });
  return { control: (action) => send.mutate(action), pending: send.isPending };
}
