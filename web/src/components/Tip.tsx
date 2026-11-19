import { Tooltip, TooltipContent, TooltipTrigger } from "@ui-sdk/react/tooltip";
import { type ReactElement, type ReactNode, useState } from "react";
import { SURFACE } from "./controls";
import { usePopupHost } from "./Select";

const TIP_DELAY_MS = 250;

type Trigger = ReactElement<Record<string, unknown>>;

/** A tip that repeats its trigger's `aria-label` names it, so a screen reader says it once. */
export function tipRole(render: Trigger, text: string): "label" | undefined {
  return render.props["aria-label"] === text ? "label" : undefined;
}

/**
 * Mounts a tooltip's content only while it is open, where `usePopupHost` says, so a closed tip
 * adds no text to its trigger's row.
 */
export function useTipMount() {
  const [host, follow] = usePopupHost();
  const [open, setOpen] = useState(false);
  const onOpenChange = (next: boolean): void => {
    follow(next);
    setOpen(next);
  };
  return { host, open, onOpenChange };
}

export function Tip({
  text,
  render,
  children,
}: {
  text: string;
  render: Trigger;
  children?: ReactNode;
}) {
  const { host, open, onOpenChange } = useTipMount();
  return (
    <Tooltip
      container={host}
      delay={TIP_DELAY_MS}
      type={tipRole(render, text)}
      onOpenChange={onOpenChange}
    >
      <TooltipTrigger render={render}>{children}</TooltipTrigger>
      {open && (
        <TooltipContent
          side="top"
          sideOffset={6}
          collisionPadding={8}
          className={`${SURFACE} nodrag nopan max-w-72 px-2 py-1 text-xs text-balance`}
        >
          {text}
        </TooltipContent>
      )}
    </Tooltip>
  );
}
