import { useCallback, useEffect, useState } from "react";
import {
  type AudioOutputs,
  listOutputs,
  onOutputsChanged,
  outputsSupported,
} from "../../lib/audio/outputs";
import type { PatchNode, SpeakerOutput } from "../../lib/types";

export const SYSTEM_OUTPUT = "System";
export const SERVER_OUTPUT = "Server";

/** The system output as a choice; every other choice is its output as JSON, so never empty. */
const SYSTEM_CHOICE = "";

export function speakerOutputOf(node: PatchNode): SpeakerOutput | null {
  return node.kind === "speaker" ? (node.data?.output ?? null) : null;
}

export function withSpeakerOutput(node: PatchNode, output: SpeakerOutput | null): PatchNode {
  return node.kind === "speaker" ? { ...node, data: { ...node.data, output } } : node;
}

/** Where the fader of the wire from `source` sits: 0 silent to 1 full. */
export function speakerVolumeOf(node: PatchNode, source: string): number {
  return node.kind === "speaker" ? (node.data?.volumes?.[source] ?? 1) : 1;
}

export function withSpeakerVolume(node: PatchNode, source: string, volume: number): PatchNode {
  if (node.kind !== "speaker") {
    return node;
  }
  const volumes = { ...node.data?.volumes, [source]: Math.min(1, Math.max(0, volume)) };
  return { ...node, data: { ...node.data, volumes } };
}

/** The device this machine plays on, or null for its default or when the server plays. */
export function listenerOutputOf(output: SpeakerOutput | null): string | null {
  return typeof output === "string" ? output : null;
}

/** The server device, null for the server's default, undefined when this machine plays. */
export function serverOutputOf(output: SpeakerOutput | null): string | null | undefined {
  return output !== null && typeof output === "object" ? (output.server ?? null) : undefined;
}

export function choiceOf(output: SpeakerOutput | null): string {
  return output === null ? SYSTEM_CHOICE : JSON.stringify(output);
}

export function outputOfChoice(choice: string): SpeakerOutput | null {
  return choice === SYSTEM_CHOICE ? null : (JSON.parse(choice) as SpeakerOutput);
}

export function serverLabel(name: string | null): string {
  return name === null ? SERVER_OUTPUT : `${SERVER_OUTPUT}: ${name}`;
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
