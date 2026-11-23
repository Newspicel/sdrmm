import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Download, RotateCw, Trash2 } from "lucide-react";
import type { ModelState } from "../lib/types";
import { Button } from "./BaseControls";
import { CHIP_SETTING, ICON_BTN_SM } from "./controls";
import { formatBytes } from "./format";
import { Icon } from "./Icon";

export type ModelStatus<M extends string> = ModelState & { model: M; bytes: number };

/** A downloadable model's control: its size to fetch, progress, a retry or a remove. */
export function ModelDownload<M extends string>({
  status,
  queryKey,
  download,
  remove,
}: {
  status: ModelStatus<M>;
  queryKey: readonly unknown[];
  download: (model: M) => Promise<void>;
  remove: (model: M) => Promise<void>;
}) {
  const queryClient = useQueryClient();
  const refresh = () => void queryClient.invalidateQueries({ queryKey });
  const fetching = useMutation({ mutationFn: download, onSettled: refresh });
  const removing = useMutation({ mutationFn: remove, onSettled: refresh });
  const size = formatBytes(status.bytes);
  switch (status.state) {
    case "ready":
      return (
        <Button
          type="button"
          className={`${ICON_BTN_SM} hover:text-danger`}
          aria-label="Remove model"
          title={`Remove model, frees ${size}`}
          onClick={() => removing.mutate(status.model)}
        >
          <Icon glyph={Trash2} size={12} />
        </Button>
      );
    case "downloading":
      return (
        <span className="legend tabular-nums">
          {Math.floor((status.received / Math.max(status.bytes, 1)) * 100)}%
        </span>
      );
    case "failed":
      return (
        <Button
          type="button"
          className={`${CHIP_SETTING} text-danger`}
          title={status.error}
          onClick={() => fetching.mutate(status.model)}
        >
          <Icon glyph={RotateCw} size={12} />
          <span className="font-sans">Retry</span>
        </Button>
      );
    case "missing":
      return (
        <Button
          type="button"
          className={CHIP_SETTING}
          title="Download model"
          disabled={fetching.isPending}
          onClick={() => fetching.mutate(status.model)}
        >
          <Icon glyph={Download} size={12} />
          <span className="font-sans">{size}</span>
        </Button>
      );
  }
}
