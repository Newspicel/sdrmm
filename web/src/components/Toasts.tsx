import { Toast, ToastClose, ToastRegion, ToastTitle } from "@ui-sdk/react/toast";
import { Flag, X } from "lucide-react";
import { type ToastData, toaster } from "../lib/toasts";
import { Button } from "./BaseControls";
import { ICON_BTN } from "./controls";
import { Icon } from "./Icon";
import { pageBody } from "./pageBody";

const STACK_LIMIT = 4;

export function Toasts({ onReport }: { onReport?: () => void }) {
  return (
    <ToastRegion
      toaster={toaster}
      container={pageBody()}
      side="bottom"
      align="end"
      maxVisible={STACK_LIMIT}
      className="fixed right-3 bottom-3 z-50 w-80 max-w-[calc(100vw-1.5rem)] *:flex *:flex-col *:gap-2"
    >
      {(toast) => {
        const error = toast.type === "error";
        const data = toast.data as ToastData | undefined;
        const repeats = data?.repeats ?? 0;
        const code = data?.code;
        return (
          <Toast
            className={`rounded-control border bg-panel-3 shadow-pop ${
              error ? "border-danger/60" : "border-line-strong"
            }`}
          >
            <div className="flex items-center gap-2 p-2 pl-3">
              <span
                className={`legend ${error ? "text-danger" : "text-ink-dim"}`}
                title={code === undefined ? undefined : `Reported by the server as \`${code}\``}
              >
                {error ? (code ?? "Error") : "Note"}
              </span>
              <ToastTitle className="min-w-0 flex-1 font-mono text-xs break-words text-ink">
                {toast.title}
                {repeats > 0 && <span className="text-ink-faint"> ×{repeats + 1}</span>}
              </ToastTitle>
              {error && onReport !== undefined && (
                <Button
                  type="button"
                  className={ICON_BTN}
                  aria-label="Report this problem"
                  title="Report this problem, with the log around it"
                  onClick={onReport}
                >
                  <Icon glyph={Flag} />
                </Button>
              )}
              <ToastClose className={ICON_BTN} aria-label="Dismiss">
                <Icon glyph={X} />
              </ToastClose>
            </div>
          </Toast>
        );
      }}
    </ToastRegion>
  );
}
