import { Collapsible, CollapsiblePanel, CollapsibleTrigger } from "@ui-sdk/react/collapsible";
import { createPortal } from "react-dom";
import { useFaultSlot } from "./faultSlot";

/** A fault on a face: one line in the footer when the face has one, else a row of its own. */
export function FaceFault({ message, detail }: { message: string; detail?: string | null }) {
  const slot = useFaultSlot();
  const more = detail == null || detail === message ? undefined : detail;
  if (slot !== null) {
    return createPortal(
      <p role="alert" title={more ?? message} className="truncate text-xs text-danger">
        {message}
      </p>,
      slot,
    );
  }
  return (
    <div role="alert" className="border-t border-line p-2 text-xs text-danger">
      {more === undefined ? (
        <p className="wrap-anywhere">{message}</p>
      ) : (
        <Collapsible>
          <CollapsibleTrigger className="cursor-pointer text-left">{message}</CollapsibleTrigger>
          <CollapsiblePanel>
            <p className="mt-1 font-mono wrap-anywhere text-ink-dim">{more}</p>
          </CollapsiblePanel>
        </Collapsible>
      )}
    </div>
  );
}
