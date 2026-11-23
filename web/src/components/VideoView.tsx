import { useEffect, useEffectEvent, useRef, useState } from "react";
import { createDrawLoop } from "../gl/drawLoop";
import { recordEvent } from "../lib/diagnostics";
import { VIDEO_RGB, type VideoFrame } from "../lib/frame";
import { latestOnly } from "../lib/latest";
import { type VideoInput, type VideoPresentation, videoHub } from "../lib/video";
import { Stat } from "./face/Stats";

const DISPLAY_ASPECT = 4 / 3;

const STALE_MS = 2_000;
const STALL_MS = 250;
const MAX_QUEUED = 64;
const MAX_QUEUED_BYTES = 24 * 1024 * 1024;
const DECODE_AHEAD_MS = 120;
const MAX_BITMAPS = 8;
const MAX_DECODES = 3;
const LATE_PICTURE_MS = 50;

interface PlaybackStats {
  fps: number;
  missing: number;
  skipped: number;
  errors: number;
  stalls: number;
  longest: number;
  frozen: number;
  drawMs: number;
}

interface Geometry {
  width: number;
  height: number;
  aspect: number;
}

export interface VideoSignal {
  width: number;
  height: number;
  live: boolean;
  stats?: PlaybackStats | null;
}

export function videoSignalText(signal: VideoSignal | null): string {
  if (signal === null) {
    return "waiting for sync";
  }
  return `${signal.width} × ${signal.height}${signal.live ? "" : " · no sync"}`;
}

function trackPlayback(input: VideoInput, onStats: (stats: PlaybackStats) => void) {
  let sampledAt = performance.now();
  let paintedAt: number | undefined;
  let rendered = 0;
  let skipped = 0;
  let errors = 0;
  let stalls = 0;
  let longest = 0;
  let drawMs = 0;
  const sample = (): void => {
    const now = performance.now();
    if (paintedAt !== undefined && !document.hidden) {
      const elapsed = now - paintedAt;
      const frozen = elapsed >= STALL_MS ? elapsed : 0;
      onStats({
        fps: (rendered * 1000) / Math.max(1, now - sampledAt),
        missing: videoHub.missingFrames(input),
        skipped: skipped + videoHub.droppedFrames(input),
        errors,
        stalls: stalls + Number(frozen > 0),
        longest: Math.max(longest, frozen),
        frozen,
        drawMs,
      });
    }
    rendered = 0;
    sampledAt = now;
  };
  const timer = window.setInterval(sample, 1000);
  return {
    presented(arrived: number): void {
      if (document.hidden) return;
      const now = performance.now();
      const gap = paintedAt === undefined ? 0 : now - paintedAt;
      if (gap >= STALL_MS) {
        stalls += 1;
        longest = Math.max(longest, gap);
      }
      paintedAt = now;
      rendered += 1;
      drawMs = now - arrived;
    },
    skip(): void {
      skipped += 1;
    },
    fail(): void {
      errors += 1;
    },
    reset(): void {
      paintedAt = undefined;
      rendered = 0;
      sampledAt = performance.now();
    },
    dispose(): void {
      clearInterval(timer);
    },
  };
}

function canvasOutput(
  canvas: HTMLCanvasElement,
  ctx: CanvasRenderingContext2D,
  setGeometry: (value: Geometry) => void,
  setLive: (value: boolean) => void,
) {
  const shown = { width: 0, height: 0, stale: 0 };
  const signal = (frame: VideoFrame): void => {
    if (shown.width !== frame.width || shown.height !== frame.height) {
      shown.width = frame.width;
      shown.height = frame.height;
      setGeometry({
        width: frame.width,
        height: frame.height,
        aspect: frame.channels === VIDEO_RGB ? frame.width / frame.height : DISPLAY_ASPECT,
      });
    }
    setLive(true);
    clearTimeout(shown.stale);
    shown.stale = window.setTimeout(() => setLive(false), STALE_MS);
  };

  const paint = (frame: VideoFrame, picture: ImageBitmap): void => {
    if (canvas.width !== frame.width || canvas.height !== frame.height) {
      canvas.width = frame.width;
      canvas.height = frame.height;
    }
    ctx.drawImage(picture, 0, 0, frame.width, frame.height);
  };

  return { signal, paint, dispose: () => clearTimeout(shown.stale) };
}

function VideoCanvas({
  canvasRef,
  aspect,
}: {
  canvasRef: React.RefObject<HTMLCanvasElement | null>;
  aspect: number;
}) {
  return (
    <div className="p-2">
      <div
        className="w-full overflow-hidden rounded-inset bg-black"
        style={{ aspectRatio: aspect }}
      >
        <canvas
          ref={canvasRef}
          aria-label="Decoded video"
          className="h-full w-full"
          style={{ imageRendering: "pixelated" }}
        />
      </div>
    </div>
  );
}

export function VideoView({
  scope,
  onSignal,
}: {
  scope: VideoInput;
  onSignal: (signal: VideoSignal | null) => void;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [geometry, setGeometry] = useState<Geometry | null>(null);
  const [live, setLive] = useState(false);
  const [stats, setStats] = useState<PlaybackStats | null>(null);
  const report = useEffectEvent(onSignal);
  const { node, port, source } = scope;

  useEffect(() => {
    report(
      geometry === null ? null : { width: geometry.width, height: geometry.height, live, stats },
    );
  }, [geometry, live, stats]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (canvas === null) {
      return;
    }
    const ctx = canvas.getContext("2d");
    if (ctx === null) {
      return;
    }
    const display = canvasOutput(canvas, ctx, setGeometry, setLive);
    let stopped = false;
    type Pending = { frame: VideoFrame; arrived: number };
    type Timed = Pending & {
      at: number;
      started: boolean;
      cancelled: boolean;
      picture: ImageBitmap | null;
    };
    const prepared: Timed[] = [];
    let decoding = 0;
    let queuedBytes = 0;
    let epoch: number | undefined;
    const discard = (job: Timed): void => {
      job.cancelled = true;
      job.picture?.close();
    };
    const dequeue = (): Timed | undefined => {
      const job = prepared.shift();
      if (job) queuedBytes -= job.frame.jpeg.byteLength;
      return job;
    };
    const clearPrepared = (): void => {
      for (const job of prepared.splice(0)) discard(job);
      queuedBytes = 0;
    };
    const pending: { frame: Pending | null } = { frame: null };
    const input = { node, port, source };
    const playback = trackPlayback(input, setStats);
    const visibilityChanged = (): void => {
      clearPrepared();
      playback.reset();
    };
    document.addEventListener("visibilitychange", visibilityChanged);

    const show = ({ frame, arrived }: Pending, picture: ImageBitmap): void => {
      if (!stopped) {
        display.paint(frame, picture);
        playback.presented(arrived);
      }
      picture.close();
    };

    const decodePicture = async (frame: VideoFrame): Promise<ImageBitmap | null> => {
      try {
        return await createImageBitmap(
          new Blob([frame.jpeg as Uint8Array<ArrayBuffer>], { type: "image/jpeg" }),
        );
      } catch {
        playback.fail();
        recordEvent("error", "video", "unreadable picture");
        return null;
      }
    };

    const decode = latestOnly(
      async (job: Pending): Promise<void> => {
        const picture = await decodePicture(job.frame);
        if (picture !== null) show(job, picture);
      },
      () => {
        playback.skip();
      },
    );

    const prepare = (): void => {
      const cutoff = performance.now() + DECODE_AHEAD_MS;
      let bitmaps = decoding + prepared.filter((entry) => entry.picture !== null).length;
      while (!stopped && decoding < MAX_DECODES && bitmaps < MAX_BITMAPS) {
        const job = prepared.find((entry) => !entry.started && entry.at <= cutoff);
        if (job === undefined) return;
        job.started = true;
        decoding += 1;
        bitmaps += 1;
        void decodePicture(job.frame).then((picture) => {
          decoding -= 1;
          if (job.cancelled || stopped) {
            picture?.close();
          } else if (picture !== null) {
            job.picture = picture;
            loop.invalidate();
          } else {
            const index = prepared.indexOf(job);
            if (index >= 0) {
              prepared.splice(index, 1);
              queuedBytes -= job.frame.jpeg.byteLength;
            }
          }
          prepare();
        });
      }
    };

    const loop = createDrawLoop(canvas, () => {
      const now = performance.now();
      if ((prepared[0]?.at ?? now) < now - LATE_PICTURE_MS) {
        const ready = prepared.findLastIndex((job) => job.picture !== null && job.at <= now + 3);
        for (let index = 0; index < ready; index += 1) {
          const old = dequeue();
          if (old) discard(old);
          playback.skip();
        }
      }
      const next = prepared[0];
      if (next?.picture && next.at <= now + 3) {
        dequeue();
        show(next, next.picture);
      } else if (prepared.length === 0 && pending.frame !== null) {
        const frame = pending.frame;
        pending.frame = null;
        decode(frame);
      }
      prepare();
      return prepared.length > 0;
    });

    const arrive = (frame: VideoFrame, presentation?: VideoPresentation): void => {
      display.signal(frame);
      const arrived = performance.now();
      if (presentation === undefined) {
        if (pending.frame !== null) playback.skip();
        pending.frame = { frame, arrived };
      } else {
        if (epoch !== presentation.epoch) {
          clearPrepared();
          epoch = presentation.epoch;
        }
        const bytes = frame.jpeg.byteLength;
        if (prepared.length >= MAX_QUEUED || queuedBytes + bytes > MAX_QUEUED_BYTES) {
          playback.skip();
          return;
        }
        queuedBytes += bytes;
        prepared.push({
          frame,
          arrived,
          at: presentation.at,
          started: false,
          cancelled: false,
          picture: null,
        });
        prepare();
      }
      loop.invalidate();
    };
    const held = videoHub.latest(input);
    if (held !== null) {
      arrive(held);
    }
    const stop = videoHub.subscribe(input, arrive);
    return () => {
      stopped = true;
      clearPrepared();
      stop();
      loop.dispose();
      display.dispose();
      playback.dispose();
      document.removeEventListener("visibilitychange", visibilityChanged);
    };
  }, [node, port, source]);

  return <VideoCanvas canvasRef={canvasRef} aspect={geometry?.aspect ?? DISPLAY_ASPECT} />;
}

export function VideoPlaybackReadouts({ stats }: { stats: PlaybackStats }) {
  return (
    <span className="contents" aria-label="Playback statistics">
      <Stat label="FPS" title="Frames drawn per second over the last update">
        {stats.fps.toFixed(1)}
      </Stat>
      <Stat
        label="Missing"
        title="Frames skipped before reaching the player, counted from stream sequence numbers"
      >
        {stats.missing}
      </Stat>
      <Stat label="Skipped" title="Frames the player received but skipped while drawing">
        {stats.skipped}
      </Stat>
      <Stat
        label="Stalls"
        title="Gaps of at least 250 ms between displayed frames"
        tone={stats.frozen ? "warn" : undefined}
      >
        {stats.stalls}
      </Stat>
      <Stat label="Longest" title="Longest displayed-frame stall">
        {Math.round(stats.longest)} ms
      </Stat>
      <Stat
        label="Draw delay"
        title="Time from delivery to the renderer until the picture appeared"
      >
        {Math.round(stats.drawMs)} ms
      </Stat>
      {stats.errors > 0 && (
        <Stat label="Decode errors" title="Images the browser could not decode" tone="warn">
          {stats.errors}
        </Stat>
      )}
      {stats.frozen > 0 && (
        <Stat label="Frozen" title="Time since the last displayed frame" tone="warn">
          {Math.round(stats.frozen)} ms
        </Stat>
      )}
    </span>
  );
}
