import {
  Select as Root,
  SelectContent,
  SelectEmpty,
  SelectInput,
  SelectItem,
  SelectList,
  SelectTrigger,
  SelectValue,
} from "@ui-sdk/react/select";
import { ChevronDown } from "lucide-react";
import { FIELD, listItem, type Options, SURFACE } from "./controls";
import { Icon } from "./Icon";
import { usePopupHost } from "./Select";
import { optionMatches } from "./selectFilter";

const TRIGGER = "w-full max-w-52";

export function SearchableSelect<T extends string | number>({
  label,
  value,
  options,
  onChange,
  className = TRIGGER,
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
      value={value}
      disabled={disabled}
      container={host}
      onOpenChange={follow}
      filter={optionMatches}
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
        className={`${SURFACE} flex max-h-[min(20rem,var(--available-height))] max-w-[calc(100vw-1rem)] min-w-(--anchor-width) flex-col`}
      >
        <SelectInput
          aria-label={`Search ${label}`}
          placeholder={`Search ${label}`}
          className={`${FIELD} m-0.5 shrink-0`}
        />
        <SelectEmpty className="px-2 py-3 text-sm text-ink-dim empty:hidden">
          Nothing matches that.
        </SelectEmpty>
        <SelectList className="flex min-h-0 flex-col overflow-y-auto overscroll-contain p-0.5">
          {options.map((option) => (
            <SelectItem
              key={String(option.value)}
              value={option.value}
              label={option.label}
              disabled={option.disabled}
              title={option.title}
              className={(state) =>
                `${listItem(state.selected, state.highlighted)} shrink-0 text-left data-disabled:opacity-45`
              }
            >
              <span className="truncate">{option.label}</span>
            </SelectItem>
          ))}
        </SelectList>
      </SelectContent>
    </Root>
  );
}
