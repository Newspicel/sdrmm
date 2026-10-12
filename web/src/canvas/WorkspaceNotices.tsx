import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Button } from "../components/BaseControls";
import { BTN_QUIET } from "../components/controls";
import { dismissNotice, WORKSPACES_KEY } from "../lib/api";
import { toastError } from "../lib/toasts";
import type { WorkspaceNotice } from "../lib/types";

const RETIRED_NAMES: Readonly<Record<string, string>> = { df: "DF" };

const SHOWN_KINDS = 4;

const CLEARED_GPS_TITLE = "This device is gone. Pick a phone.";

export interface KindNames {
  nodes: readonly { kind: string; name: string }[];
}

export interface NoticeLine {
  id: number;
  text: string;
  title: string;
}

function kindName(kind: string, catalog: KindNames): string {
  return (
    catalog.nodes.find((entry) => entry.kind === kind)?.name ?? RETIRED_NAMES[kind] ?? spoken(kind)
  );
}

function spoken(kind: string): string {
  const words = kind.replaceAll("_", " ");
  return words.charAt(0).toUpperCase() + words.slice(1);
}

export function noticeLine(notice: WorkspaceNotice, catalog: KindNames): NoticeLine {
  if (notice.kind === "cleared_gps") {
    return {
      id: notice.id,
      text: `GPS source cleared: ${notice.data.nodes.join(", ")}`,
      title: CLEARED_GPS_TITLE,
    };
  }
  if (notice.kind === "dropped_wires" || notice.kind === "added_wires") {
    const count = notice.data.wires.length;
    const wires = count === 1 ? "wire" : "wires";
    return {
      id: notice.id,
      text:
        notice.kind === "added_wires"
          ? `Added ${count} ${wires}`
          : `Removed ${count} refused ${wires}`,
      title: notice.data.wires
        .map((wire) => `${wire.from.node} to ${wire.to.node}: ${wire.reason}`)
        .join("\n"),
    };
  }
  if (notice.kind === "dropped_filter_kinds") {
    return {
      id: notice.id,
      text: `Unknown filter kinds removed: ${notice.data.kinds.join(", ")}`,
      title: notice.data.node,
    };
  }
  const names = [...new Set(notice.data.nodes.map((node) => kindName(node.kind, catalog)))];
  const more = names.length > SHOWN_KINDS ? ` +${names.length - SHOWN_KINDS}` : "";
  return {
    id: notice.id,
    text: `Removed old nodes: ${names.slice(0, SHOWN_KINDS).join(", ")}${more}`,
    title: notice.data.nodes.map((node) => node.id).join(", "),
  };
}

export function NoticeList({
  lines,
  dismissing,
  onDismiss,
}: {
  lines: readonly NoticeLine[];
  dismissing: number | null;
  onDismiss: (id: number) => void;
}) {
  if (lines.length === 0) {
    return null;
  }
  return (
    <div role="status" className="flex shrink-0 flex-col border-b border-line bg-warn/10">
      {lines.map((line) => (
        <div key={line.id} className="flex items-center gap-2 px-2 py-0.5 text-xs text-ink">
          <span title={line.title} className="min-w-0 flex-1 truncate">
            {line.text}
          </span>
          <Button
            type="button"
            className={BTN_QUIET}
            disabled={dismissing === line.id}
            onClick={() => onDismiss(line.id)}
          >
            Dismiss
          </Button>
        </div>
      ))}
    </div>
  );
}

export function WorkspaceNotices({
  workspace,
  notices,
  catalog,
}: {
  workspace: number;
  notices: readonly WorkspaceNotice[];
  catalog: KindNames;
}) {
  const queryClient = useQueryClient();
  const dismiss = useMutation({
    mutationFn: (notice: number) => dismissNotice(workspace, notice),
    onError: (error: Error) => toastError(error),
    onSettled: () => queryClient.invalidateQueries({ queryKey: [...WORKSPACES_KEY, workspace] }),
  });
  return (
    <NoticeList
      lines={notices.map((notice) => noticeLine(notice, catalog))}
      dismissing={dismiss.isPending ? (dismiss.variables ?? null) : null}
      onDismiss={(notice) => dismiss.mutate(notice)}
    />
  );
}
