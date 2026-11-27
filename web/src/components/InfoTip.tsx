import { Popover, PopoverContent, PopoverTrigger } from "@ui-sdk/react/popover";
import { Info } from "lucide-react";
import { SURFACE } from "./controls";
import { Icon } from "./Icon";
import { usePopupHost } from "./Select";

const HOVER_DELAY_MS = 250;

export function InfoTip({ text }: { text: string }) {
  const [host, follow] = usePopupHost();
  const prepare = () => follow(true);
  return (
    <Popover container={host} openOnHover delay={HOVER_DELAY_MS} onOpenChange={follow}>
      <PopoverTrigger
        aria-label={text}
        onPointerEnter={prepare}
        onFocus={prepare}
        className="nodrag nopan relative inline-flex size-3.5 touch:before:absolute touch:before:-inset-2 shrink-0 cursor-help items-center justify-center rounded-full text-ink-faint/60 hover:text-accent data-open:text-accent"
      >
        <Icon glyph={Info} size={12} />
      </PopoverTrigger>
      <PopoverContent
        side="top"
        sideOffset={6}
        collisionPadding={8}
        className={`${SURFACE} nodrag nopan max-w-72 data-ending-style:hidden px-2 py-1 text-xs text-balance`}
      >
        {text}
      </PopoverContent>
    </Popover>
  );
}
