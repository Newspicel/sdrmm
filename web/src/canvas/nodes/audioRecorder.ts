import type { AudioRecordingStatus, ChannelInfo, PatchNode, PatchNodeOf } from "../../lib/types";

export function ownRecording(channel: ChannelInfo, node: string): AudioRecordingStatus | null {
  return (channel.audio_recordings ?? []).find((status) => status.node === node) ?? null;
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
