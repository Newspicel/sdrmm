import { useEffect, useEffectEvent, useRef, useState, useSyncExternalStore } from "react";
import { recordEvent } from "../lib/diagnostics";
import { VIDEO_RGB, type VideoFrame } from "../lib/frame";
import { latestOnly } from "../lib/latest";
import { type VideoInput, videoHub } from "../lib/video";

const DISPLAY_ASPECT = 4 / 3;

const STALE_MS = 2_000;

interface Geometry {
  width: number;
  height: number;
  aspect: number;
}

export interface VideoSignal {
  width: number;
  height: number;
  live: boolean;
}

export function videoSignalText(signal: VideoSignal | null): string {
  if (signal === null) {
    return "waiting for sync";
  }
  return `${signal.width} × ${signal.height}${signal.live ? "" : " · no sync"}`;
}

export function useVideoDrops(scope: VideoInput): number {
  return useSyncExternalStore(videoHub.onDrop, () => videoHub.droppedFrames(scope));
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
  const report = useEffectEvent(onSignal);
  const { node, port, source } = scope;

  useEffect(() => {
    report(geometry === null ? null : { width: geometry.width, height: geometry.height, live });
  }, [geometry, live]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (canvas === null) {
      return;
    }
    const ctx = canvas.getContext("2d");
    if (ctx === null) {
      return;
    }
    const shown = { width: 0, height: 0, stale: 0, stopped: false };

    const paint = (frame: VideoFrame, picture: ImageBitmap): void => {
      if (shown.width !== frame.width || shown.height !== frame.height) {
        shown.width = frame.width;
        shown.height = frame.height;
        canvas.width = frame.width;
        canvas.height = frame.height;
        setGeometry({
          width: frame.width,
          height: frame.height,
          aspect: frame.channels === VIDEO_RGB ? frame.width / frame.height : DISPLAY_ASPECT,
        });
      }
      ctx.drawImage(picture, 0, 0, frame.width, frame.height);
      setLive(true);
      clearTimeout(shown.stale);
      shown.stale = window.setTimeout(() => setLive(false), STALE_MS);
    };

    const draw = latestOnly(async (frame: VideoFrame): Promise<void> => {
      try {
        const picture = await createImageBitmap(
          new Blob([frame.jpeg as Uint8Array<ArrayBuffer>], { type: "image/jpeg" }),
        );
        if (!shown.stopped) {
          paint(frame, picture);
        }
        picture.close();
      } catch {
        recordEvent("error", "video", "unreadable picture");
      }
    });

    const input = { node, port, source };
    const held = videoHub.latest(input);
    if (held !== null) {
      draw(held);
    }
    const stop = videoHub.subscribe(input, draw);
    return () => {
      shown.stopped = true;
      stop();
      clearTimeout(shown.stale);
    };
  }, [node, port, source]);

  return (
    <div className="p-2">
      <div
        className="w-full overflow-hidden rounded-inset bg-black"
        style={{ aspectRatio: geometry?.aspect ?? DISPLAY_ASPECT }}
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
