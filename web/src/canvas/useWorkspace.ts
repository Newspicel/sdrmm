import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";
import {
  activateWorkspace,
  applyWorkspace,
  cloneWorkspace,
  createWorkspace,
  deleteWorkspace,
  importWorkspace,
  putWorkspaceChannel,
  STATE_KEY,
  stepWorkspace,
  updateWorkspace,
  WORKSPACES_KEY,
  workspaceQuery,
  workspacesQuery,
} from "../lib/api";
import { toastError } from "../lib/toasts";
import type {
  ChannelSettings,
  WorkspaceDetail,
  WorkspaceInfo,
  WorkspaceSnapshot,
} from "../lib/types";
import { retryNodeState } from "../lib/useNodeStateSync";
import { pruneRack } from "./graph";
import { savedChannelsOf, withSavedChannel } from "./savedChannels";
import { useSwitchStore } from "./switches";
import { type Edit, newer, WorkspaceEdits } from "./workspaceEdits";
import { parseWorkspaceExport } from "./workspaceExport";

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
  saveChannel: (node: string, settings: ChannelSettings) => void;
}

export function useWorkspace(): WorkspaceStore {
  const queryClient = useQueryClient();
  const list = useQuery(workspacesQuery());
  const activeId = list.data?.active ?? null;
  const detail = useQuery(workspaceQuery(activeId));

  const [edits] = useState(() => new WorkspaceEdits(fitRack));
  const [, bump] = useReducer((count: number) => count + 1, 0);

  const store = useCallback(
    (stored: WorkspaceDetail) =>
      queryClient.setQueryData<WorkspaceDetail>([...WORKSPACES_KEY, stored.id], (cached) =>
        newer(cached, stored),
      ),
    [queryClient],
  );
  const update = useMutation({
    mutationFn: (variables: { id: number; revision: number; snapshot: WorkspaceSnapshot }) =>
      updateWorkspace(variables.id, {
        revision: variables.revision,
        snapshot: variables.snapshot,
      }),
    onSuccess: store,
  });
  const renameMut = useMutation({
    mutationFn: (variables: { id: number; name: string }) =>
      updateWorkspace(variables.id, {
        revision:
          queryClient.getQueryData<WorkspaceDetail>([...WORKSPACES_KEY, variables.id])?.revision ??
          0,
        name: variables.name,
      }),
    onSuccess: store,
    onSettled: () => queryClient.invalidateQueries({ queryKey: WORKSPACES_KEY }),
  });
  const cloneMut = useMutation({
    mutationFn: cloneWorkspace,
    onSettled: () => queryClient.invalidateQueries({ queryKey: WORKSPACES_KEY }),
  });
  const activateMut = useMutation({
    mutationFn: (id: number) => {
      useSwitchStore.getState().expect(id);
      return activateWorkspace(id);
    },
    onSuccess: (report) => {
      useSwitchStore.getState().publish(report);
      retryNodeState(queryClient);
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: WORKSPACES_KEY }),
  });
  const createMut = useMutation({
    mutationFn: (name: string) => createWorkspace(name),
    onSuccess: (id) => activateMut.mutate(id),
    onSettled: () => queryClient.invalidateQueries({ queryKey: WORKSPACES_KEY }),
  });
  const importMut = useMutation({
    mutationFn: async (file: File) => importWorkspace(parseWorkspaceExport(await file.text())),
    onSuccess: (id) => activateMut.mutate(id),
    onSettled: () => queryClient.invalidateQueries({ queryKey: WORKSPACES_KEY }),
  });
  const removeMut = useMutation({
    mutationFn: deleteWorkspace,
    onSettled: () => queryClient.invalidateQueries({ queryKey: WORKSPACES_KEY }),
  });
  const applyMut = useMutation({
    mutationFn: applyWorkspace,
    onSuccess: (report) => {
      useSwitchStore.getState().publish(report);
      retryNodeState(queryClient);
    },
  });
  const applyAsync = applyMut.mutateAsync;
  const stepMut = useMutation({
    mutationFn: (variables: { id: number; step: "undo" | "redo" }) =>
      stepWorkspace(variables.id, variables.step),
    onSuccess: (stepped, variables) => {
      queryClient.setQueryData<WorkspaceDetail>([...WORKSPACES_KEY, variables.id], stepped);
      void queryClient.invalidateQueries({ queryKey: STATE_KEY });
    },
  });
  const stepAsync = stepMut.mutateAsync;

  const queried = detail.data ?? null;
  const savedChannels = useMemo(() => savedChannelsOf(queried), [queried]);
  const saveChannelMut = useMutation({
    mutationFn: (variables: { id: number; node: string; settings: ChannelSettings }) =>
      putWorkspaceChannel(variables.id, variables.node, variables.settings),
    onError: (error: Error) => toastError(error),
    onSettled: (_data, _error, variables) =>
      void queryClient.invalidateQueries({ queryKey: [...WORKSPACES_KEY, variables.id] }),
  });
  const saveChannelAsync = saveChannelMut.mutate;
  const saveChannel = useCallback(
    (node: string, settings: ChannelSettings): void => {
      if (activeId === null) {
        return;
      }
      queryClient.setQueryData<WorkspaceDetail>([...WORKSPACES_KEY, activeId], (held) =>
        held === undefined ? held : withSavedChannel(held, node, settings),
      );
      saveChannelAsync({ id: activeId, node, settings });
    },
    [activeId, queryClient, saveChannelAsync],
  );
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
    error:
      errorOf(update.error) ??
      errorOf(createMut.error) ??
      errorOf(renameMut.error) ??
      errorOf(cloneMut.error) ??
      errorOf(importMut.error) ??
      errorOf(removeMut.error) ??
      errorOf(applyMut.error) ??
      errorOf(stepMut.error),
    save,
    activate: activateMut.mutate,
    create: createMut.mutate,
    rename,
    clone,
    importFile: importMut.mutate,
    remove: removeMut.mutate,
    apply,
    undo,
    redo,
    canUndo: queried?.history?.can_undo ?? false,
    canRedo: queried?.history?.can_redo ?? false,
    pending: list.isPending || (activeId !== null && detail.isPending),
    unreachable: errorOf(list.error),
    savedChannels,
    saveChannel,
  };
}

function errorOf(error: Error | null): string | null {
  return error === null ? null : error.message;
}

function fitRack(snapshot: WorkspaceSnapshot): WorkspaceSnapshot {
  const rack = pruneRack(snapshot.rack ?? {}, snapshot.graph);
  return rack === snapshot.rack ? snapshot : { ...snapshot, rack };
}
