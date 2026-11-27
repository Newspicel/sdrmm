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
  openAt: (clientX: number, clientY: number) => void;
  pickChannel: () => void;
  closeMenu: () => void;
  closePicker: () => void;
}

export function useScopePicks(input: {
  plotRef: RefObject<HTMLDivElement | null>;
  active: boolean;
  meta: FrameMeta | null;
  view: SpectrumView;
}): ScopePicks {
  const { plotRef, active, meta, view } = input;
  const [menu, setMenu] = useState<Opened | null>(null);
  const [picker, setPicker] = useState<{ pick: ScopePick; frame: string } | null>(null);
  const frame = `${meta?.centerHz}:${meta?.spanHz}:${view.start}:${view.end}`;
  const open = menu?.frame === frame ? menu : null;

  const openAt = (clientX: number, clientY: number): void => {
    const rect = plotRef.current?.getBoundingClientRect();
    if (!active || meta === null || rect === undefined || rect.width === 0) {
      return;
    }
    const at = (clientX - rect.left) / rect.width;
    setMenu({
      pick: pickAt(meta.centerHz, meta.spanHz, view, at),
      at: { x: at, y: rect.height === 0 ? 0 : (clientY - rect.top) / rect.height },
      frame,
    });
  };

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
      openAt(event.clientX, event.clientY);
    },
    openAt,
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
