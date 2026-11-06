import { useCallback, useEffect, useState } from "react";
import {
  type AudioOutputs,
  listOutputs,
  onOutputsChanged,
  outputsSupported,
} from "../../lib/audio/outputs";
import type { PatchNode } from "../../lib/types";

export const SYSTEM_OUTPUT = "System";

export function speakerOutputOf(node: PatchNode): string | null {
  return node.kind === "speaker" ? (node.data ?? null) : null;
}

export function withSpeakerOutput(node: PatchNode, output: string | null): PatchNode {
  return node.kind === "speaker" ? { ...node, data: output } : node;
}

export function outputMissing(output: string | null, listed: AudioOutputs | null): boolean {
  return (
    output !== null &&
    listed !== null &&
    listed.named &&
    !listed.outputs.some((candidate) => candidate.name === output)
  );
}

export interface ListedOutputs {
  readonly listed: AudioOutputs | null;
  readonly refresh: () => void;
}

export function useAudioOutputs(): ListedOutputs {
  const [listed, setListed] = useState<AudioOutputs | null>(null);
  const refresh = useCallback(() => {
    if (!outputsSupported()) {
      return;
    }
    listOutputs()
      .then(setListed)
      .catch((err: unknown) => console.error("listing audio outputs failed:", err));
  }, []);
  useEffect(() => {
    refresh();
    return onOutputsChanged(refresh);
  }, [refresh]);
  return { listed, refresh };
}
