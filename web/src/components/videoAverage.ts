import type { DbWindow } from "./spectrumTraces";

export const AVERAGE_CHOICES = [1, 2, 4, 8, 16] as const;
export type AverageFrames = (typeof AVERAGE_CHOICES)[number];
export const DEFAULT_AVERAGE: AverageFrames = 4;

export class VideoAverage {
  private out = new Float32Array(0);
  private primed = false;

  reset(): void {
    this.primed = false;
  }

  apply(db: Float32Array, frames: number): Float32Array {
    if (frames <= 1) {
      this.primed = false;
      return db;
    }
    if (db.length !== this.out.length) {
      this.out = new Float32Array(db.length);
      this.primed = false;
    }
    const weight = this.primed ? 1 / frames : 1;
    for (let i = 0; i < db.length; i++) {
      const level = db[i] ?? 0;
      const held = this.out[i] ?? level;
      this.out[i] = held + (level - held) * weight;
    }
    this.primed = true;
    return this.out;
  }
}

export function quantizeDb(db: Float32Array, window: DbWindow, out: Uint8Array | null): Uint8Array {
  const dst = out !== null && out.length === db.length ? out : new Uint8Array(db.length);
  const span = window.max - window.min;
  if (!(span > 0)) {
    dst.fill(0);
    return dst;
  }
  const scale = 255 / span;
  for (let i = 0; i < db.length; i++) {
    dst[i] = Math.min(255, Math.max(0, Math.round(((db[i] ?? 0) - window.min) * scale)));
  }
  return dst;
}
