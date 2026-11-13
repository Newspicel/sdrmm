import { type QueryClient, useQueryClient } from "@tanstack/react-query";
import { patchNodeChannel, STATE_KEY, WORKSPACES_KEY } from "./api";
import { createChannelQueue } from "./channelQueue";
import { channelSettingsAt, editedChannel, showChannel } from "./nodeCache";
import { toastError } from "./toasts";
import type { ChannelSettings } from "./types";

export type ChannelEdit =
  | Partial<ChannelSettings>
  | ((current: ChannelSettings) => Partial<ChannelSettings>);

type Queue = ReturnType<typeof createChannelQueue>;

const queues = new WeakMap<QueryClient, Queue>();

function queueFor(queryClient: QueryClient): Queue {
  const held = queues.get(queryClient);
  if (held !== undefined) {
    return held;
  }
  const queue = createChannelQueue({
    read: (node) => channelSettingsAt(queryClient, node),
    show: (node, settings) => showChannel(queryClient, node, settings),
    send: patchNodeChannel,
    failed: (error) => toastError(error),
    drained: () => {
      void queryClient.invalidateQueries({ queryKey: STATE_KEY });
      void queryClient.invalidateQueries({ queryKey: WORKSPACES_KEY });
    },
  });
  queues.set(queryClient, queue);
  return queue;
}

export function useChannelPatch(): {
  applyEdit: (node: string, edit: ChannelEdit) => void;
} {
  const queryClient = useQueryClient();
  const applyEdit = (node: string, edit: ChannelEdit): void => {
    void queryClient.cancelQueries({ queryKey: STATE_KEY });
    queueFor(queryClient)(node, (current) => editedChannel(current, edit));
  };
  return { applyEdit };
}
