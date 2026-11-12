import { describe, expect, it } from "vitest";
import {
  encodeMicrophonePcm,
  FrameChunker,
  MAX_MICROPHONE_FRAME,
  MICROPHONE_FRAME_SAMPLES,
  peakDb,
  peakOf,
  VoxGate,
} from "./microphone";

describe("encodeMicrophonePcm", () => {
  it("writes clamped 16-bit little-endian samples as base64", () => {
    expect(encodeMicrophonePcm(new Float32Array([0, 1]))).toBe("AAD/fw==");
    const bytes = Uint8Array.from(atob(encodeMicrophonePcm(new Float32Array([0.5, -1, 2]))), (c) =>
      c.charCodeAt(0),
    );
    const view = new DataView(bytes.buffer);
    expect(bytes.length).toBe(6);
    expect(view.getInt16(0, true)).toBe(16_384);
    expect(view.getInt16(2, true)).toBe(-32_767);
    expect(view.getInt16(4, true)).toBe(32_767);
  });

  it("never sends more than the server takes", () => {
    const encoded = encodeMicrophonePcm(new Float32Array(MAX_MICROPHONE_FRAME + 10));
    expect(atob(encoded).length).toBe(2 * MAX_MICROPHONE_FRAME);
  });
});

const collect = (rate: number, blocks: Float32Array[]): Float32Array[] => {
  const frames: Float32Array[] = [];
  const chunker = new FrameChunker(rate, (frame) => frames.push(frame));
  for (const block of blocks) {
    chunker.push(block);
  }
  return frames;
};

describe("FrameChunker", () => {
  it("passes 48 kHz through one sample late in whole frames whatever the block size", () => {
    const ramp = Float32Array.from({ length: 2_400 }, (_, n) => n / 2_400);
    const blocks = [ramp.subarray(0, 128), ramp.subarray(128, 1_000), ramp.subarray(1_000)];
    const frames = collect(48_000, blocks);
    expect(frames.map((frame) => frame.length)).toEqual([
      MICROPHONE_FRAME_SAMPLES,
      MICROPHONE_FRAME_SAMPLES,
    ]);
    expect(frames[0]?.[100]).toBeCloseTo(99 / 2_400, 6);
    expect(frames[1]?.[0]).toBeCloseTo(959 / 2_400, 6);
  });

  it("resamples another rate to 48 kHz and keeps the tone", () => {
    const tone = Float32Array.from({ length: 44_100 }, (_, n) =>
      Math.sin((2 * Math.PI * 1_000 * n) / 44_100),
    );
    const frames = collect(44_100, [tone.subarray(0, 20_000), tone.subarray(20_000)]);
    expect(frames.length).toBe(50);
    const joined = frames.flatMap((frame) => [...frame]);
    const crossings = joined.filter(
      (sample, n) => n > 0 && (joined[n - 1] ?? 0) < 0 && sample >= 0,
    );
    expect(crossings.length).toBeGreaterThan(995);
    expect(crossings.length).toBeLessThan(1_005);
  });
});

describe("peakOf", () => {
  it("is the largest magnitude", () => {
    expect(peakOf(new Float32Array([0.1, -0.7, 0.3]))).toBeCloseTo(0.7, 6);
    expect(peakOf(new Float32Array())).toBe(0);
  });
});

describe("VoxGate", () => {
  it("opens at the threshold and not below it", () => {
    const gate = new VoxGate(-20, 500);
    expect(gate.feed(0.05, 0)).toBe(false);
    expect(gate.feed(0.1, 20)).toBe(true);
  });

  it("holds for the hang time, closes after it and opens again", () => {
    const gate = new VoxGate(-20, 500);
    expect(gate.feed(0.5, 0)).toBe(true);
    expect(gate.feed(0, 500)).toBe(true);
    expect(gate.feed(0, 501)).toBe(false);
    expect(gate.feed(0.5, 900)).toBe(true);
    expect(gate.feed(0.01, 1_000)).toBe(true);
  });

  it("stays shut on silence and reads silence as the floor", () => {
    const gate = new VoxGate(-90, 5_000);
    expect(gate.feed(0, 0)).toBe(false);
    expect(peakDb(0)).toBe(-120);
    expect(peakDb(1)).toBe(0);
    expect(peakDb(0.1)).toBeCloseTo(-20, 6);
  });
});
