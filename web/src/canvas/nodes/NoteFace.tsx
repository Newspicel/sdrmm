import { Pencil } from "lucide-react";
import { useRef, useState } from "react";
import { Button, Textarea } from "../../components/BaseControls";
import { ICON_BTN_SM } from "../../components/controls";
import { Icon } from "../../components/Icon";
import { Markdown } from "../../components/Markdown";
import { MAX_NOTE_LEN } from "../../lib/limits";
import type { PatchNode, PatchNodeOf } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { FaceBody, FaceEmpty, NodeShell } from "./NodeShell";

const HINT = "Double-click to write markdown";

export function NoteFace({ node }: { node: PatchNode }) {
  if (node.kind !== "note") {
    return null;
  }
  return <Face node={node} />;
}

function Face({ node }: { node: PatchNodeOf<"note"> }) {
  const workspace = useWorkspaceContext();
  const text = node.data.text;
  const [draft, setDraft] = useState<string | null>(null);
  const kept = useRef(true);

  const start = () => {
    kept.current = true;
    setDraft(text);
  };

  const finish = () => {
    setDraft(null);
    if (!kept.current || draft === null || draft === text) {
      return;
    }
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        current.kind === "note" ? { ...current, data: { text: draft } } : current,
      ),
    }));
  };

  return (
    <NodeShell
      node={node}
      title="Note"
      category="tool"
      actions={
        draft === null && (
          <Button
            type="button"
            className={ICON_BTN_SM}
            aria-label="Edit note"
            title={HINT}
            onClick={start}
          >
            <Icon glyph={Pencil} size={12} />
          </Button>
        )
      }
    >
      <FaceBody>
        {draft === null ? (
          <div className="flex min-h-0 flex-1 flex-col" title={HINT} onDoubleClick={start}>
            {text.trim() === "" ? <FaceEmpty hint={HINT} /> : <Markdown text={text} />}
          </div>
        ) : (
          <Textarea
            autoFocus
            className="nodrag nopan min-h-0 flex-1 resize-none bg-transparent px-2.5 py-2 font-mono text-xs leading-snug text-ink outline-none"
            aria-label="Note text"
            placeholder="# Heading, **bold**, - bullets, [links](https://example.com)"
            maxLength={MAX_NOTE_LEN}
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            onBlur={finish}
            onKeyDown={(event) => {
              event.stopPropagation();
              if (event.key === "Escape") {
                kept.current = false;
                event.currentTarget.blur();
              }
            }}
          />
        )}
      </FaceBody>
    </NodeShell>
  );
}
