import { type RefObject, useState } from "react";
import type { SpectrumView } from "../../components/spectrumView";
import type { ScopeMenuAt } from "./ScopeMenu";
import type { FrameMeta } from "./scopeFeed";
import { onPlotSurface } from "./scopeGesture";
import { pickAt, type ScopePick } from "./scopePick";

interface Opened {
  pick: ScopePick;
  at: ScopeMenuAt;
  frame: string;
}

export interface ScopePicks {
  menu: Opened | null;
  picker: ScopePick | null;
  onContextMenu: (event: React.MouseEvent<HTMLDivElement>) => void;
  pickChannel: () => void;
  closeMenu: () => void;
  closePicker: () => void;
}

export function useScopePicks(input: {
  plotRef: RefObject<HTMLDivElement | null>;
  active: boolean;
  meta: FrameMeta | null;
  view: SpectrumView;
  pointerFraction: (clientX: number) => number;
}): ScopePicks {
  const { plotRef, active, meta, view, pointerFraction } = input;
  const [menu, setMenu] = useState<Opened | null>(null);
  const [picker, setPicker] = useState<{ pick: ScopePick; frame: string } | null>(null);
  const frame = `${meta?.centerHz}:${meta?.spanHz}:${view.start}:${view.end}`;
  const open = menu?.frame === frame ? menu : null;

  return {
    menu: open,
    picker: picker?.frame === frame ? picker.pick : null,
    onContextMenu: (event) => {
      const plot = plotRef.current;
      if (!active || meta === null || plot === null || !onPlotSurface(event.target, plot)) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      const rect = plot.getBoundingClientRect();
      const at = pointerFraction(event.clientX);
      setMenu({
        pick: pickAt(meta.centerHz, meta.spanHz, view, at),
        at: { x: at, y: rect.height === 0 ? 0 : (event.clientY - rect.top) / rect.height },
        frame,
      });
    },
    pickChannel: () => {
      if (open !== null) {
        setPicker({ pick: open.pick, frame });
      }
      setMenu(null);
    },
    closeMenu: () => setMenu(null),
    closePicker: () => setPicker(null),
  };
}
