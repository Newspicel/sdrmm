import { Collapsible, CollapsiblePanel, CollapsibleTrigger } from "@ui-sdk/react/collapsible";

export function FaceFault({ message, detail }: { message: string; detail?: string | null }) {
  return (
    <div role="alert" className="border-t border-line p-2 text-xs text-danger">
      {detail == null || detail === message ? (
        <p className="wrap-anywhere">{message}</p>
      ) : (
        <Collapsible>
          <CollapsibleTrigger className="cursor-pointer text-left">{message}</CollapsibleTrigger>
          <CollapsiblePanel>
            <p className="mt-1 font-mono wrap-anywhere text-ink-dim">{detail}</p>
          </CollapsiblePanel>
        </Collapsible>
      )}
    </div>
  );
}
