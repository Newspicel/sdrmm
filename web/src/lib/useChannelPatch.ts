import { useMutation, useQueryClient } from "@tanstack/react-query";
import { patchNodeChannel, STATE_KEY, WORKSPACES_KEY } from "./api";
import { channelSettingsAt, editedChannel, showChannel } from "./nodeCache";
import { toastError } from "./toasts";
import type { ChannelSettings } from "./types";

export type ChannelEdit =
  | Partial<ChannelSettings>
  | ((current: ChannelSettings) => Partial<ChannelSettings>);

export function useChannelPatch(): {
  applyEdit: (node: string, edit: ChannelEdit) => void;
} {
  const queryClient = useQueryClient();
  const patchMut = useMutation({
    mutationFn: (v: { node: string; settings: ChannelSettings; base: ChannelSettings }) =>
      patchNodeChannel(v.node, v.settings, v.base),
    onError: (error) => toastError(error),
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: STATE_KEY });
      void queryClient.invalidateQueries({ queryKey: WORKSPACES_KEY });
    },
  });

  const applyEdit = (node: string, edit: ChannelEdit): void => {
    void queryClient.cancelQueries({ queryKey: STATE_KEY });
    const current = channelSettingsAt(queryClient, node);
    if (current === undefined) {
      return;
    }
    const settings = editedChannel(current, edit);
    showChannel(queryClient, node, settings);
    patchMut.mutate({ node, settings, base: current });
  };

  return { applyEdit };
}
