import { type QueryClient, useMutation } from "@tanstack/react-query";
import {
  activateWorkspace,
  applyWorkspace,
  cloneWorkspace,
  createWorkspace,
  deleteWorkspace,
  importWorkspace,
  STATE_KEY,
  stepWorkspace,
  updateWorkspace,
  WORKSPACES_KEY,
} from "../lib/api";
import type { WorkspaceDetail, WorkspaceSnapshot } from "../lib/types";
import { retryNodeState } from "../lib/useNodeStateSync";
import { useSwitchStore } from "./switches";
import { newer } from "./workspaceEdits";
import { parseWorkspaceExport } from "./workspaceExport";

export function useWorkspaceMutations(queryClient: QueryClient) {
  const store = (stored: WorkspaceDetail) =>
    queryClient.setQueryData<WorkspaceDetail>([...WORKSPACES_KEY, stored.id], (cached) =>
      newer(cached, stored),
    );
  const listed = () => queryClient.invalidateQueries({ queryKey: WORKSPACES_KEY });
  const revisionOf = (id: number) =>
    queryClient.getQueryData<WorkspaceDetail>([...WORKSPACES_KEY, id])?.revision ?? 0;

  const update = useMutation({
    mutationFn: (variables: { id: number; revision: number; snapshot: WorkspaceSnapshot }) =>
      updateWorkspace(variables.id, {
        revision: variables.revision,
        snapshot: variables.snapshot,
      }),
    onSuccess: store,
  });
  const rename = useMutation({
    mutationFn: (variables: { id: number; name: string }) =>
      updateWorkspace(variables.id, { revision: revisionOf(variables.id), name: variables.name }),
    onSuccess: store,
    onSettled: listed,
  });
  const clone = useMutation({ mutationFn: cloneWorkspace, onSettled: listed });
  const activate = useMutation({
    mutationFn: (id: number) => {
      useSwitchStore.getState().expect(id);
      return activateWorkspace(id);
    },
    onSuccess: (report) => {
      useSwitchStore.getState().publish(report);
      retryNodeState(queryClient);
    },
    onSettled: listed,
  });
  const create = useMutation({
    mutationFn: (name: string) => createWorkspace(name),
    onSuccess: (id) => activate.mutate(id),
    onSettled: listed,
  });
  const importFile = useMutation({
    mutationFn: async (file: File) => importWorkspace(parseWorkspaceExport(await file.text())),
    onSuccess: (id) => activate.mutate(id),
    onSettled: listed,
  });
  const remove = useMutation({ mutationFn: deleteWorkspace, onSettled: listed });
  const apply = useMutation({
    mutationFn: applyWorkspace,
    onSuccess: (report) => {
      useSwitchStore.getState().publish(report);
      retryNodeState(queryClient);
    },
  });
  const step = useMutation({
    mutationFn: (variables: { id: number; step: "undo" | "redo" }) =>
      stepWorkspace(variables.id, variables.step),
    onSuccess: (stepped, variables) => {
      queryClient.setQueryData<WorkspaceDetail>([...WORKSPACES_KEY, variables.id], stepped);
      void queryClient.invalidateQueries({ queryKey: STATE_KEY });
    },
  });

  return {
    update,
    rename,
    clone,
    activate,
    create,
    importFile,
    remove,
    apply,
    step,
    error:
      messageOf(update.error) ??
      messageOf(create.error) ??
      messageOf(rename.error) ??
      messageOf(clone.error) ??
      messageOf(importFile.error) ??
      messageOf(remove.error) ??
      messageOf(apply.error) ??
      messageOf(step.error),
  };
}

function messageOf(error: Error | null): string | null {
  return error === null ? null : error.message;
}
