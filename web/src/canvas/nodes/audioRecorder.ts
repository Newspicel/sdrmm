import type { AudioRecordingStatus, ChannelInfo, PatchNode, PatchNodeOf } from "../../lib/types";

/** The take `node` records from a channel or from a stream a node makes itself. */
export function ownRecording(
  source: Pick<ChannelInfo, "audio_recordings"> | null,
  node: string,
): AudioRecordingStatus | null {
  return (source?.audio_recordings ?? []).find((status) => status.node === node) ?? null;
}

export type RecorderNodeOf = PatchNodeOf<"recorder" | "audio_recorder" | "baseband_recorder">;

export function withRecording(node: PatchNode, recording: boolean): PatchNode {
  if (node.kind === "recorder") {
    return { ...node, data: { ...node.data, recording } };
  }
  if (node.kind === "audio_recorder") {
    return { ...node, data: { ...node.data, recording } };
  }
  if (node.kind === "baseband_recorder") {
    return { ...node, data: { ...node.data, recording } };
  }
  return node;
}

export function withSkipSilence(node: PatchNode, skip_silence: boolean): PatchNode {
  return node.kind === "audio_recorder" ? { ...node, data: { ...node.data, skip_silence } } : node;
}
