import { createPopoverController, Popover, PopoverContent } from "@ui-sdk/react/popover";
import { useReactFlow } from "@xyflow/react";
import { useEffect, useState } from "react";
import { SURFACE } from "../components/controls";
import { usePopupHost } from "../components/Select";
import { NodePalette } from "./NodePalette";
import { useAddNode } from "./useAddNode";

export interface ScreenPoint {
  x: number;
  y: number;
}

/**
 * Opens from code after mount rather than rendering open: a popover rendered open leaves focus
 * alone, and the palette's search field should take it.
 */
function useOpenOnMount() {
  const [controller] = useState(() => createPopoverController());
  useEffect(() => {
    controller.setOpen(true);
  }, [controller]);
  return controller;
}

export function CanvasPalette({ at, onClose }: { at: ScreenPoint; onClose: () => void }) {
  const add = useAddNode();
  const { screenToFlowPosition } = useReactFlow();
  const controller = useOpenOnMount();
  const [host, follow] = usePopupHost();

  return (
    <Popover
      controller={controller}
      container={host}
      onOpenChange={(open) => {
        follow(open);
        if (!open) {
          onClose();
        }
      }}
    >
      <PopoverContent
        anchor={at}
        side="bottom"
        align="start"
        collisionPadding={8}
        className={`${SURFACE} w-[44rem] data-ending-style:hidden max-w-[calc(100vw-1rem)]`}
      >
        <NodePalette
          onAdd={(kind, channelType) => {
            add(kind, channelType, screenToFlowPosition(at));
            onClose();
          }}
        />
      </PopoverContent>
    </Popover>
  );
}
