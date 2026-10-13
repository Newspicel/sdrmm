import { useQueryClient } from "@tanstack/react-query";
import { nodeAction, STATE_KEY } from "./api";
import { toastError } from "./toasts";
import { createTuneQueue } from "./useArrayTune";

const queue = createTuneQueue((node, hz) =>
  nodeAction(node, { action: "tune", with: { freq_hz: hz } }),
);

export function useNodeTune(): (node: string, hz: number) => void {
  const queryClient = useQueryClient();
  return (node, hz) =>
    queue(node, hz, (error, idle) => {
      if (error !== null) {
        toastError(error);
      }
      if (idle) {
        void queryClient.invalidateQueries({ queryKey: STATE_KEY });
      }
    });
}
