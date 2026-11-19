import { useQuery } from "@tanstack/react-query";
import {
  Dialog,
  DialogBackdrop,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@ui-sdk/react/dialog";
import { Suspense, useRef } from "react";
import { BTN, DIALOG_TITLE, SURFACE } from "../components/controls";
import { PortalContainerProvider } from "../components/PortalContainer";
import { pageBody } from "../components/pageBody";
import { toolsQuery } from "../lib/api";
import { findTool, type LaunchableTool, launchableTools, toolSize } from "./registry";

const SIZES = {
  standard: "max-h-[80vh] w-full max-w-3xl",
  full: "h-[94vh] w-[97vw] max-w-none",
} as const;

export function ToolsDialog({ tool, onClose }: { tool: string | null; onClose: () => void }) {
  const tools = useQuery(toolsQuery());
  const active = findTool(launchableTools(tools.data?.tools ?? []), tool);
  const size = SIZES[toolSize(tool)];
  const portalContainer = useRef<HTMLDivElement>(null);

  return (
    <Dialog
      container={pageBody()}
      open={tool !== null}
      onOpenChange={(open) => {
        if (!open) {
          onClose();
        }
      }}
    >
      <DialogBackdrop className="fixed inset-0 z-40 bg-bg/85 data-ending-style:hidden" />
      <DialogContent
        ref={portalContainer}
        className={`${SURFACE} fixed top-1/2 data-ending-style:hidden left-1/2 z-40 flex ${size} -translate-x-1/2 -translate-y-1/2 flex-col`}
      >
        <PortalContainerProvider container={portalContainer}>
          <div className="flex shrink-0 items-baseline justify-between gap-4 border-b border-line px-4 py-3">
            <DialogTitle className={DIALOG_TITLE}>{active?.descriptor.name ?? "Tool"}</DialogTitle>
            {active === null && (
              <DialogDescription className="legend">
                This build no longer offers that tool
              </DialogDescription>
            )}
          </div>

          <div className="min-h-0 flex-1 overflow-auto p-4">
            {active !== null && <ToolBody tool={active} />}
          </div>

          <div className="flex shrink-0 justify-end border-t border-line px-4 py-3">
            <DialogClose className={BTN}>Close</DialogClose>
          </div>
        </PortalContainerProvider>
      </DialogContent>
    </Dialog>
  );
}

function ToolBody({ tool }: { tool: LaunchableTool }) {
  const Panel = tool.panel;
  if (Panel === null) {
    return (
      <p className="text-xs text-ink-dim">
        The server offers <span className="text-ink">{tool.descriptor.name}</span>, but this client
        has no panel for it. It is still reachable over the API.
      </p>
    );
  }
  return (
    <Suspense fallback={<p className="legend">Loading…</p>}>
      <Panel key={tool.descriptor.id} />
    </Suspense>
  );
}
