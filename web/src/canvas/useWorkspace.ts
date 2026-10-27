import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";
import { WORKSPACES_KEY, workspaceQuery, workspacesQuery } from "../lib/api";
import type {
  ChannelSettings,
  WorkspaceDetail,
  WorkspaceInfo,
  WorkspaceSnapshot,
} from "../lib/types";
import { pruneRack } from "./graph";
import { savedChannelsOf } from "./savedChannels";
import { type Edit, WorkspaceEdits } from "./workspaceEdits";
import { useWorkspaceMutations } from "./workspaceMutations";

export interface WorkspaceStore {
  workspaces: WorkspaceInfo[];
  active: WorkspaceDetail | null;
  error: string | null;
  save: (edit: Edit) => void;
  activate: (id: number) => void;
  create: (name: string) => void;
  rename: (id: number, name: string) => void;
  clone: (id: number) => void;
  importFile: (file: File) => void;
  remove: (id: number) => void;
  apply: () => void;
  undo: () => void;
  redo: () => void;
  canUndo: boolean;
  canRedo: boolean;
  pending: boolean;
  unreachable: string | null;
  savedChannels: ReadonlyMap<string, ChannelSettings>;
}

export function useWorkspace(): WorkspaceStore {
  const queryClient = useQueryClient();
  const list = useQuery(workspacesQuery());
  const activeId = list.data?.active ?? null;
  const detail = useQuery(workspaceQuery(activeId));

  const [edits] = useState(() => new WorkspaceEdits(fitRack));
  const [, bump] = useReducer((count: number) => count + 1, 0);

  const mutations = useWorkspaceMutations(queryClient);
  const { update, apply: applyMut, step: stepMut, rename: renameMut, clone: cloneMut } = mutations;
  const applyAsync = applyMut.mutateAsync;
  const stepAsync = stepMut.mutateAsync;

  const queried = detail.data ?? null;
  const savedChannels = useMemo(() => savedChannelsOf(queried), [queried]);
  const active = queried === null ? null : edits.view(queried);
  const activeIdRef = useRef<number | null>(null);
  useLayoutEffect(() => {
    activeIdRef.current = active?.id ?? null;
  });
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const finishQueue = useCallback((task: Promise<unknown>) => {
    queue.current = task;
  }, []);

  const flush = useCallback(
    (id: number) => {
      const key = [...WORKSPACES_KEY, id] as const;
      const task = queue.current
        .catch(() => undefined)
        .then(async () => {
          const base = queryClient.getQueryData<WorkspaceDetail>(key);
          const outgoing = base === undefined ? null : edits.take(id, base);
          if (outgoing === null) {
            return;
          }
          try {
            await update.mutateAsync({ id, ...outgoing });
          } catch {
            await queryClient.invalidateQueries({ queryKey: key });
          } finally {
            edits.landed(id);
            bump();
          }
        })
        .catch(() => undefined);
      finishQueue(task);
    },
    [edits, finishQueue, queryClient, update],
  );

  const save = useCallback(
    (edit: Edit) => {
      const id = activeIdRef.current;
      if (id === null) {
        return;
      }
      edits.push(id, edit);
      bump();
      flush(id);
    },
    [edits, flush],
  );

  const apply = useCallback(() => {
    const id = activeIdRef.current;
    if (id === null) {
      return;
    }
    const task = queue.current
      .catch(() => undefined)
      .then(() => applyAsync(id))
      .catch(() => undefined);
    finishQueue(task);
  }, [applyAsync, finishQueue]);

  const step = useCallback(
    (which: "undo" | "redo") => {
      const id = activeIdRef.current;
      if (id === null) {
        return;
      }
      const task = queue.current
        .catch(() => undefined)
        .then(() => stepAsync({ id, step: which }))
        .catch(() => undefined);
      finishQueue(task);
    },
    [finishQueue, stepAsync],
  );
  const undo = useCallback(() => step("undo"), [step]);
  const redo = useCallback(() => step("redo"), [step]);

  const renameAsync = renameMut.mutateAsync;
  const rename = useCallback(
    (id: number, name: string) => {
      const task = queue.current
        .catch(() => undefined)
        .then(() => renameAsync({ id, name }))
        .catch(() => undefined);
      finishQueue(task);
    },
    [finishQueue, renameAsync],
  );

  const cloneAsync = cloneMut.mutateAsync;
  const clone = useCallback(
    (id: number) => {
      const task = queue.current
        .catch(() => undefined)
        .then(() => cloneAsync(id))
        .catch(() => undefined);
      finishQueue(task);
    },
    [cloneAsync, finishQueue],
  );

  const broughtUp = useRef(false);
  const loaded = active?.id ?? null;
  useEffect(() => {
    if (loaded !== null && !broughtUp.current) {
      broughtUp.current = true;
      apply();
    }
  }, [loaded, apply]);

  return {
    workspaces: list.data?.workspaces ?? [],
    active,
    error: mutations.error,
    save,
    activate: mutations.activate.mutate,
    create: mutations.create.mutate,
    rename,
    clone,
    importFile: mutations.importFile.mutate,
    remove: mutations.remove.mutate,
    apply,
    undo,
    redo,
    canUndo: queried?.history?.can_undo ?? false,
    canRedo: queried?.history?.can_redo ?? false,
    pending: list.isPending || (activeId !== null && detail.isPending),
    unreachable: errorOf(list.error),
    savedChannels,
  };
}

function errorOf(error: Error | null): string | null {
  return error === null ? null : error.message;
}

function fitRack(snapshot: WorkspaceSnapshot): WorkspaceSnapshot {
  const rack = pruneRack(snapshot.rack ?? {}, snapshot.graph);
  return rack === snapshot.rack ? snapshot : { ...snapshot, rack };
}
