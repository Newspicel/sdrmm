export const MICROPHONE_RATE_HZ = 48_000;
export const MICROPHONE_FRAME_SAMPLES = 960;
export const MAX_MICROPHONE_FRAME = 4_800;

/** One frame's samples as a `Speak` command carries them: base64 of 16-bit little-endian mono. */
export function encodeMicrophonePcm(pcm: Float32Array): string {
  const samples = pcm.subarray(0, MAX_MICROPHONE_FRAME);
  const bytes = new Uint8Array(2 * samples.length);
  const view = new DataView(bytes.buffer);
  samples.forEach((sample, index) => {
    const clamped = Math.min(1, Math.max(-1, sample));
    view.setInt16(2 * index, Math.round(clamped * 32_767), true);
  });
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary);
}

export function peakOf(pcm: Float32Array): number {
  let peak = 0;
  for (const sample of pcm) {
    peak = Math.max(peak, Math.abs(sample));
  }
  return peak;
}

const SILENT_DB = -120;

export function peakDb(peak: number): number {
  return peak > 0 ? Math.max(SILENT_DB, 20 * Math.log10(peak)) : SILENT_DB;
}

/** The voice key: open from the first frame at the threshold until the hang time after the last. */
export class VoxGate {
  private loudAt: number | null = null;

  constructor(
    private readonly thresholdDb: number,
    private readonly hangMs: number,
  ) {}

  feed(peak: number, nowMs: number): boolean {
    if (peakDb(peak) >= this.thresholdDb) {
      this.loudAt = nowMs;
      return true;
    }
    return this.loudAt !== null && nowMs - this.loudAt <= this.hangMs;
  }
}

/** Brings capture blocks at any rate to whole 20 ms frames at the uplink rate. */
export class FrameChunker {
  private readonly step: number;
  private readonly frame = new Float32Array(MICROPHONE_FRAME_SAMPLES);
  private filled = 0;
  private position = 0;
  private last = 0;

  constructor(
    inputRate: number,
    private readonly onFrame: (frame: Float32Array) => void,
  ) {
    this.step = inputRate / MICROPHONE_RATE_HZ;
  }

  push(block: Float32Array): void {
    while (this.position < block.length) {
      const index = Math.floor(this.position);
      const before = index === 0 ? this.last : (block[index - 1] ?? 0);
      const at = block[index] ?? 0;
      this.emit(before + (at - before) * (this.position - index));
      this.position += this.step;
    }
    this.position -= block.length;
    this.last = block[block.length - 1] ?? this.last;
  }

  private emit(sample: number): void {
    this.frame[this.filled] = sample;
    this.filled += 1;
    if (this.filled === this.frame.length) {
      this.filled = 0;
      this.onFrame(this.frame.slice());
    }
  }
}
