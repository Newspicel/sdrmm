import {
  Select as Root,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@ui-sdk/react/select";
import { ChevronDown } from "lucide-react";
import { useState } from "react";
import { CONTROL_W, FIELD, listItem, type Options, SURFACE } from "./controls";
import { Icon } from "./Icon";
import { usePortalContainer } from "./PortalContainer";

/**
 * Where a popup mounts: the surrounding portal container, else the body. In place, the trigger's
 * parent would style it through child selectors. Read on open, as the ref is empty on first render.
 */
export function usePopupHost(): readonly [Element | undefined, (open: boolean) => void] {
  const portal = usePortalContainer();
  const [host, setHost] = useState<Element>();
  const follow = (open: boolean): void => {
    if (open) {
      setHost(portal?.current ?? document.body);
    }
  };
  return [host, follow];
}

export function Select<T extends string | number>({
  label,
  value,
  options,
  onChange,
  className = CONTROL_W,
  disabled = false,
}: {
  label: string;
  value: T;
  options: Options<T>;
  onChange: (value: T) => void;
  className?: string;
  disabled?: boolean;
}) {
  const [host, follow] = usePopupHost();
  return (
    <Root
      items={[...options]}
      value={value}
      disabled={disabled}
      container={host}
      onOpenChange={follow}
      onValueChange={(next) => {
        if (next !== null) {
          onChange(next);
        }
      }}
    >
      <SelectTrigger
        data-hotkeys="off"
        aria-label={label}
        className={`${FIELD} justify-between ${className}`}
      >
        <SelectValue className="truncate" />
        <span aria-hidden className="shrink-0 text-ink-faint">
          <Icon glyph={ChevronDown} size={12} />
        </span>
      </SelectTrigger>
      <SelectContent
        data-hotkeys="off"
        side="bottom"
        align="start"
        sideOffset={4}
        className={`${SURFACE} flex max-h-(--available-height) max-w-[calc(100vw-1rem)] min-w-(--anchor-width) flex-col overflow-y-auto p-0.5`}
      >
        {options.map((option) => (
          <SelectItem
            key={String(option.value)}
            value={option.value}
            disabled={option.disabled}
            title={option.title}
            className={(state) =>
              `${listItem(state.selected, state.highlighted)} shrink-0 data-disabled:opacity-45`
            }
          >
            {option.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Root>
  );
}
