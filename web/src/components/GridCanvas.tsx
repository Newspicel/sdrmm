import { useEffect, useLayoutEffect, useRef } from "react";
import { createDrawLoop, type DrawLoop } from "../gl/drawLoop";
import { attachGrid, type Colormap, type GridFrame } from "../gl/surface";

export function GridCanvas({
  frame,
  colormap,
  flipY,
  transparentFloor = false,
  className,
}: {
  frame: GridFrame | null;
  colormap: Colormap;
  flipY: boolean;
  transparentFloor?: boolean;
  className?: string;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const latest = useRef({ frame, colormap });
  const loopRef = useRef<DrawLoop | null>(null);
  useLayoutEffect(() => {
    latest.current = { frame, colormap };
  });

  useEffect(() => {
    const canvas = ref.current;
    if (canvas === null) {
      return;
    }
    const view = attachGrid(canvas, { flipY, transparentFloor });
    const loop = createDrawLoop(canvas, () => {
      const shown = latest.current;
      if (shown.frame !== null) {
        view.setColormap(shown.colormap);
        view.draw(shown.frame);
      }
      return false;
    });
    loopRef.current = loop;
    return () => {
      loop.dispose();
      view.dispose();
      loopRef.current = null;
    };
  }, [flipY, transparentFloor]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: a new frame or colormap is what asks for the paint
  useEffect(() => {
    loopRef.current?.invalidate();
    // oxlint-disable-next-line react/exhaustive-effect-dependencies -- a new frame or colormap is what asks for the paint
  }, [frame, colormap]);

  return <canvas ref={ref} aria-hidden className={className ?? "absolute inset-0 size-full"} />;
}
