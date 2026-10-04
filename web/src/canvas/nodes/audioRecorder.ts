import type { AudioRecordingStatus, ChannelInfo, PatchNode, PatchNodeOf } from "../../lib/types";

function sameRoute(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((node, index) => node === b[index]);
}

export function recordingFor(
  channel: ChannelInfo,
  fx: readonly string[],
): AudioRecordingStatus | null {
  return (channel.audio_recordings ?? []).find((status) => sameRoute(status.fx ?? [], fx)) ?? null;
}

export type RecorderNodeOf = PatchNodeOf<"recorder" | "audio_recorder" | "baseband_recorder">;

function isRecorder(node: PatchNode): node is RecorderNodeOf {
  return (
    node.kind === "recorder" || node.kind === "audio_recorder" || node.kind === "baseband_recorder"
  );
}

export function withRecording(node: PatchNode, recording: boolean): PatchNode {
  if (node.kind === "audio_recorder") {
    return { ...node, data: { ...node.data, recording } };
  }
  return isRecorder(node) ? { ...node, data: { ...node.data, recording } } : node;
}

export function withSkipSilence(node: PatchNode, skip_silence: boolean): PatchNode {
  return node.kind === "audio_recorder" ? { ...node, data: { ...node.data, skip_silence } } : node;
}
