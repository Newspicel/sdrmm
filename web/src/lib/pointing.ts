import { useMutation } from "@tanstack/react-query";
import { controlSkyScan } from "./api";
import { clearAction, failAction } from "./refusals";
import { statusStore } from "./statusStore";
import type { AimStatus, SkyScanStatus, SurveyAction } from "./types";

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
