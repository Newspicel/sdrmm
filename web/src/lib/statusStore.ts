import { create } from "zustand";
import type { ServerEvent } from "./types";

interface StatusStore<T> {
  byNode: Readonly<Record<string, T>>;
  observe: (event: ServerEvent) => void;
  put: (status: T) => void;
  reset: () => void;
}

export function statusStore<T extends { node: string }>(pick: (event: ServerEvent) => T | null) {
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
