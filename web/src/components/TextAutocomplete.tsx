import {
  Autocomplete,
  AutocompleteContent,
  AutocompleteEmpty,
  AutocompleteInput,
  AutocompleteItem,
  AutocompleteList,
  AutocompleteTrigger,
} from "@ui-sdk/react/autocomplete";
import { ChevronDown } from "lucide-react";
import type { ReactElement } from "react";
import { useState } from "react";
import { commitText, FIELD, SURFACE } from "./controls";
import { Icon } from "./Icon";
import { usePopupHost } from "./Select";

export interface AutocompleteSuggestion {
  value: string;
  detail?: string;
}

export function TextAutocomplete({
  label,
  value,
  suggestions,
  placeholder,
  inputMode,
  className,
  onCommit,
}: {
  label: string;
  value: string;
  suggestions: readonly AutocompleteSuggestion[];
  placeholder?: string;
  inputMode?: "decimal" | "numeric" | "search" | "text";
  className?: string;
  onCommit: (value: string) => boolean;
}) {
  const [draft, setDraft] = useDraft(value);
  const [open, setOpen] = useState(false);
  const [host, follow] = usePopupHost();

  const show = (next: boolean): void => {
    follow(next);
    setOpen(next);
  };

  const commit = (candidate: string): void => {
    setDraft(commitText(candidate, value, onCommit));
  };

  return (
    <Autocomplete
      value={draft}
      filter={false}
      container={host}
      open={open}
      onOpenChange={show}
      onSubmit={commit}
      onValueChange={(next, details) => {
        setDraft(next);
        if (details.reason === "item-press") {
          commit(next);
        }
      }}
    >
      <div className={`relative min-w-0 ${className ?? ""}`}>
        <AutocompleteInput
          data-hotkeys="off"
          aria-label={label}
          inputMode={inputMode}
          placeholder={placeholder}
          className={`${FIELD} w-full pr-7`}
          onClick={() => show(true)}
          onBlur={(event) => commit(event.currentTarget.value)}
        />
        <AutocompleteTrigger
          data-hotkeys="off"
          aria-label={`Show ${label.toLowerCase()} suggestions`}
          className="absolute inset-y-0 right-0 flex w-7 cursor-pointer items-center justify-center text-ink-faint hover:text-ink"
        >
          <Icon glyph={ChevronDown} size={12} />
        </AutocompleteTrigger>
      </div>
      <AutocompleteContent
        data-hotkeys="off"
        side="bottom"
        align="start"
        sideOffset={4}
        className={`${SURFACE} flex max-h-(--available-height) max-w-[calc(100vw-1rem)] min-w-(--anchor-width) flex-col`}
      >
        <AutocompleteEmpty className="text-xs text-ink-dim empty:hidden">
          <span className="block px-2 py-1.5">
            No detected match. A custom value is still accepted.
          </span>
        </AutocompleteEmpty>
        <AutocompleteList className="flex min-h-0 flex-col overflow-y-auto p-0.5">
          {suggestions.map((suggestion) => suggestionItem(suggestion, value))}
        </AutocompleteList>
      </AutocompleteContent>
    </Autocomplete>
  );
}

export function suggestionItem(suggestion: AutocompleteSuggestion, chosen: string): ReactElement {
  return (
    <AutocompleteItem
      key={suggestion.value}
      value={suggestion.value}
      label={suggestion.value}
      className={(state) =>
        "flex shrink-0 cursor-default flex-col items-start rounded-control px-2 py-1 text-xs " +
        "leading-tight " +
        (suggestion.value === chosen
          ? "bg-accent/15 text-accent"
          : state.highlighted
            ? "bg-panel-2 text-ink"
            : "text-ink")
      }
    >
      <span className="font-mono">{suggestion.value}</span>
      {suggestion.detail !== undefined && <span className="opacity-60">{suggestion.detail}</span>}
    </AutocompleteItem>
  );
}

function useDraft(value: string): [string, (value: string) => void] {
  const [draft, setDraft] = useState(value);
  const [seen, setSeen] = useState(value);
  if (seen !== value) {
    setSeen(value);
    setDraft(value);
    return [value, setDraft];
  }
  return [draft, setDraft];
}
