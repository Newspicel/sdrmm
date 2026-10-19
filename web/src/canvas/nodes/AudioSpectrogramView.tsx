import { useEffect, useRef, useState } from "react";
import { rowLevel } from "../../components/spectrumAlign";
import { type Colormap, DEFAULT_COLORMAP } from "../../gl/colormap";
import { attachWaterfall, GRAPHICS_HELP, type WaterfallView } from "../../gl/waterfall";
import {
  AUDIO_SPECTRUM_BINS,
  AUDIO_SPECTRUM_FPS,
  audioNyquistHz,
  spectrumHub,
} from "../../lib/spectrum";

const TICKS_HZ = [3000, 6000, 12_000, 18_000];

export function AudioSpectrogramView({
  node,
  port,
  source,
  playing,
  colormap = DEFAULT_COLORMAP,
}: {
  node: string;
  port: string;
  source: string;
  playing: boolean;
  colormap?: Colormap;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rendererRef = useRef<WaterfallView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (canvas === null) {
      return;
    }
    let renderer: WaterfallView;
    try {
      renderer = attachWaterfall(canvas, setError);
    } catch (thrown) {
      // oxlint-disable-next-line react/set-state-in-effect -- the WebGL context is the external system this effect attaches, and its refusal has to reach the face
      setError(thrown instanceof Error ? thrown.message : String(thrown));
      return;
    }
    renderer.setColormap(colormap);
    rendererRef.current = renderer;
    return () => {
      renderer.dispose();
      rendererRef.current = null;
    };
  }, [colormap]);

  useEffect(() => {
    if (!playing) {
      return;
    }
    return spectrumHub.subscribe(
      node,
      port,
      (frame) => {
        rendererRef.current?.pushRow(
          frame.bins,
          rowLevel({ min: frame.dbMin, max: frame.dbMax }, frame.floorDb, Date.now()),
        );
      },
      { bins: AUDIO_SPECTRUM_BINS, fps: AUDIO_SPECTRUM_FPS, source },
    );
  }, [node, port, source, playing]);

  return (
    <div className="relative h-24 w-full overflow-hidden rounded-control bg-plot-bg">
      <canvas ref={canvasRef} className="h-full w-full" />

      {TICKS_HZ.map((hz) => (
        <span
          key={hz}
          className="pointer-events-none absolute bottom-0.5 legend font-mono text-2xs -translate-x-1/2 rounded-inset bg-plot-bg/85 px-0.5 leading-none text-plot-ink"
          style={{ left: `${(hz / audioNyquistHz()) * 100}%` }}
        >
          {`${(hz / 1000).toFixed(0)}k`}
        </span>
      ))}

      {error !== null && (
        <span className="pointer-events-none absolute inset-0 flex items-center justify-center px-2">
          <span
            className="pointer-events-auto text-center legend font-mono text-2xs text-danger"
            title={GRAPHICS_HELP}
          >
            {error}
          </span>
        </span>
      )}
    </div>
  );
}
