import { PopoverContent, PopoverTrigger, Popover as Primitive } from "@ui-sdk/react/popover";
import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import { SURFACE } from "./controls";
import { usePopupHost } from "./Select";
import { Tip } from "./Tip";

const HOVER_DELAY_MS = 120;

export const CANVAS_MOVED = "sdrmm:canvas-moved";

export function Popover({
  label,
  triggerClass,
  align = "start",
  width = "w-80",
  padded = true,
  disabled = false,
  openOnHover = false,
  title,
  children,
}: {
  label: ReactNode;
  triggerClass: string;
  align?: "start" | "end";
  width?: string;
  padded?: boolean;
  disabled?: boolean;
  openOnHover?: boolean;
  title?: string;
  children: (close: () => void) => ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [host, follow] = usePopupHost();
  useEffect(() => {
    if (!open) {
      return;
    }
    const close = () => setOpen(false);
    window.addEventListener(CANVAS_MOVED, close);
    return () => window.removeEventListener(CANVAS_MOVED, close);
  }, [open]);
  const triggerProps = { className: triggerClass, disabled, "aria-label": title };

  return (
    <Primitive
      container={host}
      open={open}
      onOpenChange={(next) => {
        follow(next);
        setOpen(next);
      }}
      openOnHover={openOnHover}
      delay={HOVER_DELAY_MS}
    >
      {title === undefined || openOnHover ? (
        <PopoverTrigger {...triggerProps}>{label}</PopoverTrigger>
      ) : (
        <Tip text={title} render={<PopoverTrigger {...triggerProps} />}>
          {label}
        </Tip>
      )}
      <PopoverContent
        side="bottom"
        align={align}
        sideOffset={4}
        className={`${width} ${SURFACE} data-ending-style:hidden max-h-[var(--available-height)] max-w-[calc(100vw-1rem)] overflow-y-auto ${padded ? "p-3" : ""}`}
      >
        {children(() => setOpen(false))}
      </PopoverContent>
    </Primitive>
  );
}
