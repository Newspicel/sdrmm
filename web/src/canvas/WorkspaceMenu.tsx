import { Copy, Download, Pencil, X } from "lucide-react";
import { useRef, useState } from "react";
import { Button, Form, Input } from "../components/BaseControls";
import {
  BTN_DANGER_SM,
  BTN_QUIET,
  BTN_SM,
  FIELD,
  ICON_BTN_SM,
  LABEL,
  listItem,
} from "../components/controls";
import { Icon } from "../components/Icon";
import { workspaceExportUrl } from "../lib/api";
import { useT } from "../lib/i18n/translate";
import { MAX_NAME_LEN } from "../lib/limits";
import { pickFile } from "../lib/pickFile";
import type { WorkspaceInfo } from "../lib/types";

import { WORKSPACE_FILE_ACCEPT } from "./workspaceExport";

export function WorkspaceMenu({
  workspaces,
  activeWorkspace,
  others,
  onActivate,
  onCreate,
  onRename,
  onClone,
  onImport,
  onRemove,
}: {
  workspaces: readonly WorkspaceInfo[];
  activeWorkspace: number | null;
  others: number;
  onActivate: (id: number) => void;
  onCreate: (name: string) => void;
  onRename: (id: number, name: string) => void;
  onClone: (id: number) => void;
  onImport: (file: File) => void;
  onRemove: (id: number) => void;
}) {
  const t = useT();
  const [name, setName] = useState("");
  return (
    <div className="flex flex-col gap-1">
      <span className={LABEL}>{t("workspace.list")}</span>
      {workspaces.map((workspace) => (
        <WorkspaceRow
          key={workspace.id}
          workspace={workspace}
          active={workspace.id === activeWorkspace}
          others={others}
          onActivate={onActivate}
          onRename={onRename}
          onClone={onClone}
          onRemove={onRemove}
        />
      ))}
      <div className="mt-1 flex flex-col gap-1 border-t border-line pt-2">
        <Form
          className="flex gap-1"
          onSubmit={(event) => {
            event.preventDefault();
            if (name.trim() !== "") {
              onCreate(name.trim());
              setName("");
            }
          }}
        >
          <Input
            className={`${FIELD} flex-1`}
            aria-label={t("workspace.newName")}
            placeholder={t("workspace.newPlaceholder")}
            maxLength={MAX_NAME_LEN}
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
          <Button type="submit" className={BTN_QUIET} disabled={name.trim() === ""}>
            {t("workspace.add")}
          </Button>
        </Form>
        <Button
          type="button"
          className={`${BTN_QUIET} justify-center`}
          title={t("workspace.importTitle")}
          onClick={() => {
            void pickFile(WORKSPACE_FILE_ACCEPT).then((file) => {
              if (file !== null) {
                onImport(file);
              }
            });
          }}
        >
          {t("workspace.import")}
        </Button>
      </div>
    </div>
  );
}

function WorkspaceRow({
  workspace,
  active,
  others,
  onActivate,
  onRename,
  onClone,
  onRemove,
}: {
  workspace: WorkspaceInfo;
  active: boolean;
  others: number;
  onActivate: (id: number) => void;
  onRename: (id: number, name: string) => void;
  onClone: (id: number) => void;
  onRemove: (id: number) => void;
}) {
  const t = useT();
  const named = { name: workspace.name };
  const [mode, setMode] = useState<"idle" | "rename" | "confirm" | "switch">("idle");
  const [draft, setDraft] = useState(workspace.name);
  const abandoned = useRef(false);

  if (mode === "rename") {
    const finish = () => {
      setMode("idle");
      const next = draft.trim();
      if (!abandoned.current && next !== "" && next !== workspace.name) {
        onRename(workspace.id, next);
      }
    };
    return (
      <Input
        autoFocus
        className={`${FIELD} w-full`}
        aria-label={t("workspace.renameField", named)}
        maxLength={MAX_NAME_LEN}
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onFocus={(event) => event.currentTarget.select()}
        onBlur={finish}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.currentTarget.blur();
          }
          if (event.key === "Escape") {
            event.stopPropagation();
            abandoned.current = true;
            event.currentTarget.blur();
          }
        }}
      />
    );
  }

  if (mode === "confirm") {
    return (
      <div className="flex h-7 items-center gap-1 rounded-control border border-danger bg-danger/10 px-2">
        <span className="min-w-0 flex-1 truncate text-xs text-danger">
          {t("workspace.deleteAsk", named)}
        </span>
        <Button
          type="button"
          className={BTN_DANGER_SM}
          title={t("workspace.deleteTitle")}
          onClick={() => {
            setMode("idle");
            onRemove(workspace.id);
          }}
        >
          {t("workspace.delete")}
        </Button>
        <Button autoFocus type="button" className={BTN_SM} onClick={() => setMode("idle")}>
          {t("workspace.keep")}
        </Button>
      </div>
    );
  }

  if (mode === "switch") {
    return (
      <div className="flex h-7 items-center gap-1 rounded-control border border-accent bg-accent/10 px-2">
        <span className="min-w-0 flex-1 truncate text-xs">
          {others === 1 ? t("workspace.switchOne") : t("workspace.switchMany", { count: others })}
        </span>
        <Button
          type="button"
          className={BTN_SM}
          title={t("workspace.switchTitle")}
          onClick={() => {
            setMode("idle");
            onActivate(workspace.id);
          }}
        >
          {t("workspace.switch")}
        </Button>
        <Button autoFocus type="button" className={BTN_SM} onClick={() => setMode("idle")}>
          {t("workspace.stay")}
        </Button>
      </div>
    );
  }

  return (
    <div className="group flex items-center gap-1">
      <Button
        type="button"
        className={`${listItem(active, false)} min-w-0 flex-1`}
        aria-pressed={active}
        onClick={() => {
          if (active) {
            return;
          }
          if (others > 0) {
            setMode("switch");
          } else {
            onActivate(workspace.id);
          }
        }}
      >
        <span className="truncate">{workspace.name}</span>
      </Button>
      <span
        aria-hidden
        className="w-4 text-right font-mono text-2xs text-ink-faint tabular-nums"
        title={t("workspace.nodes", { count: workspace.nodes })}
      >
        {workspace.nodes}
      </span>
      <span className="ml-1 flex items-center gap-0.5 opacity-0 transition-opacity duration-100 group-focus-within:opacity-100 group-hover:opacity-100">
        <Button
          type="button"
          className={ICON_BTN_SM}
          aria-label={t("workspace.rename", named)}
          title={t("workspace.renameTitle")}
          onClick={() => {
            abandoned.current = false;
            setDraft(workspace.name);
            setMode("rename");
          }}
        >
          <Icon glyph={Pencil} size={12} />
        </Button>
        <Button
          type="button"
          className={ICON_BTN_SM}
          aria-label={t("workspace.duplicate", named)}
          title={t("workspace.duplicateTitle")}
          onClick={() => onClone(workspace.id)}
        >
          <Icon glyph={Copy} size={12} />
        </Button>
        <a
          className={ICON_BTN_SM}
          href={workspaceExportUrl(workspace.id)}
          download
          aria-label={t("workspace.export", named)}
          title={t("workspace.exportTitle")}
        >
          <Icon glyph={Download} size={12} />
        </a>
        <Button
          type="button"
          className={ICON_BTN_SM}
          aria-label={t("workspace.deleteNamed", named)}
          title={t("workspace.deleteThis")}
          onClick={() => setMode("confirm")}
        >
          <Icon glyph={X} size={12} />
        </Button>
      </span>
    </div>
  );
}
