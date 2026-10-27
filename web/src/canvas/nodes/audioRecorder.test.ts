import { describe, expect, it } from "vitest";
import type { AudioRecordingStatus, ChannelInfo, PatchNode } from "../../lib/types";
import { ownRecording, withRecording, withSkipSilence } from "./audioRecorder";

const status = (file: string, node: string): AudioRecordingStatus => ({
  node,
  file,
  started_at: "2026-09-23T12:00:00Z",
  channels: 1,
  frames: 0,
  bytes: 0,
});

const channel = (audio_recordings?: AudioRecordingStatus[]): ChannelInfo => ({
  id: 1,
  settings: { frequency_hz: 1, params: { type: "nfm", settings: {} } },
  audio_recordings,
});

describe("ownRecording", () => {
  it("finds the file this recorder owns on a channel others record too", () => {
    const wired = channel([status("mine.wav", "rec"), status("theirs.wav", "other")]);
    expect(ownRecording(wired, "rec")?.file).toBe("mine.wav");
    expect(ownRecording(wired, "other")?.file).toBe("theirs.wav");
  });

  it("is null while this recorder records nothing there", () => {
    expect(ownRecording(channel([status("theirs.wav", "other")]), "rec")).toBeNull();
    expect(ownRecording(channel(), "rec")).toBeNull();
  });
});

const placed = { id: "rec", position: { x: 0, y: 0 } };

describe("recorder edits", () => {
  it("switching an audio recorder keeps its silence choice", () => {
    const quiet: PatchNode = {
      ...placed,
      kind: "audio_recorder",
      data: { recording: false, skip_silence: true },
    };
    expect(withRecording(quiet, true)).toMatchObject({
      data: { recording: true, skip_silence: true },
    });
    expect(withSkipSilence(quiet, false)).toMatchObject({
      data: { recording: false, skip_silence: false },
    });
  });

  it("only audio recorders skip silence", () => {
    const iq: PatchNode = {
      ...placed,
      kind: "recorder",
      data: { recording: false, pre_roll_s: 0 },
    };
    expect(withSkipSilence(iq, true)).toBe(iq);
    expect(withRecording(iq, true)).toMatchObject({ data: { recording: true } });
  });
});
