import { create } from "zustand";
import type { PatchApplyReport, ServerEvent, WorkspacesResponse } from "../lib/types";

export interface Switched {
  id: number;
  by: string | null;
  mine: boolean;
}

export interface SwitchState {
  report: PatchApplyReport | null;
  expected: ReadonlySet<number>;
  publish: (report: PatchApplyReport) => void;
  expect: (id: number) => void;
  observe: (event: ServerEvent) => Switched | null;
}

export const useSwitchStore = create<SwitchState>((set, get) => ({
  report: null,
  expected: new Set(),
  publish: (report) => set({ report }),
  expect: (id) => set((state) => ({ expected: new Set([...state.expected, id]) })),
  observe: (event) => {
    if (event.type !== "WorkspaceSwitched") {
      return null;
    }
    const { id, by, report } = event.data;
    const mine = get().expected.has(id);
    set((state) => ({
      report,
      expected: new Set([...state.expected].filter((held) => held !== id)),
    }));
    return { id, by: by ?? null, mine };
  },
}));

export function switchNotice(switched: Switched, list: WorkspacesResponse | undefined): string {
  const name = list?.workspaces.find((workspace) => workspace.id === switched.id)?.name;
  return `${switched.by ?? "Someone"} switched to ${name ?? "another workspace"}`;
}
