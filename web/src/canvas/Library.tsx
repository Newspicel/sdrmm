import { Tabs, TabsList, TabsPanel, TabsTab } from "@ui-sdk/react/tabs";
import type { LucideIcon } from "lucide-react";
import {
  AudioLines,
  Bookmark,
  Disc3,
  KeyRound,
  LayoutTemplate,
  Shapes,
  SlidersHorizontal,
  Smartphone,
  Wrench,
} from "lucide-react";
import { Fragment, type ReactNode } from "react";
import { AccountsPanel } from "../components/AccountsPanel";
import { BandsPanel } from "../components/BandsPanel";
import { BookmarksPanel } from "../components/BookmarksPanel";
import { ExamplesPanel } from "../components/ExamplesPanel";
import { Icon } from "../components/Icon";
import { PresetsPanel } from "../components/PresetsPanel";
import { RecordingsPanel } from "../components/RecordingsPanel";
import { TemplatesPanel } from "../components/TemplatesPanel";
import { oneOf, rememberedKey, useRemembered } from "../lib/remembered";
import type { RecordingInfo } from "../lib/types";
import { ToolsPanel } from "../tools/ToolsPanel";
import { useWorkspaceContext } from "./context";
import { addNode, newNodeId, nodeIds } from "./graph";
import { libraryTarget } from "./libraryTarget";
import { recordingNodeFor } from "./nodes/recordingNode";
import { PhonesPanel } from "./PhonesPanel";
import { useNodePlacement } from "./placement";

type SectionId =
  | "templates"
  | "examples"
  | "presets"
  | "bookmarks"
  | "bands"
  | "recordings"
  | "tools"
  | "phones"
  | "accounts";

interface Section {
  id: SectionId;
  label: string;
  glyph: LucideIcon;
}

/** Sections in groups; a rule between groups is the only heading they get. */
const GROUPS: readonly (readonly Section[])[] = [
  [
    { id: "templates", label: "Templates", glyph: LayoutTemplate },
    { id: "examples", label: "Examples", glyph: Shapes },
    { id: "presets", label: "Presets", glyph: SlidersHorizontal },
  ],
  [
    { id: "bookmarks", label: "Bookmarks", glyph: Bookmark },
    { id: "bands", label: "Bands", glyph: AudioLines },
  ],
  [
    { id: "recordings", label: "Recordings", glyph: Disc3 },
    { id: "tools", label: "Tools", glyph: Wrench },
  ],
  [
    { id: "phones", label: "Phones", glyph: Smartphone },
    { id: "accounts", label: "Accounts", glyph: KeyRound },
  ],
];

const SECTION_IDS = GROUPS.flat().map((section) => section.id);
const SECTION_KEY = rememberedKey("librarySection");
const IS_SECTION = oneOf(SECTION_IDS);

const RAIL =
  "flex w-36 shrink-0 flex-col gap-px overflow-y-auto border-r border-line bg-panel p-1.5";
const PANE = "h-[30rem] min-w-0 flex-1 overflow-y-auto";

function railItem(selected: boolean): string {
  return (
    "relative flex h-7 w-full items-center gap-2 rounded-control px-2 text-xs outline-none " +
    "transition-colors duration-100 focus-visible:ring-2 focus-visible:ring-accent " +
    (selected
      ? "bg-accent/15 font-medium text-ink before:absolute before:inset-y-1.5 before:left-0 " +
        "before:w-0.5 before:rounded-full before:bg-accent"
      : "text-ink-dim hover:bg-panel-2 hover:text-ink")
  );
}

export function Library({
  onOpenTool,
  onActivate,
}: {
  onOpenTool: (id: string) => void;
  onActivate: (id: number) => void;
}) {
  const [section, setSection] = useRemembered<SectionId>(SECTION_KEY, "templates", IS_SECTION);
  const panes = useSectionPanes(onOpenTool, onActivate);
  return (
    <Tabs
      orientation="vertical"
      value={section}
      onValueChange={(next) => next !== null && setSection(next)}
      className="flex overflow-hidden rounded-control"
    >
      <TabsList className={RAIL} aria-label="Library section">
        {GROUPS.map((group, index) => (
          <Fragment key={group[0]?.id}>
            {index > 0 && <span aria-hidden className="mx-2 my-1 h-px shrink-0 bg-line" />}
            {group.map((entry) => (
              <TabsTab
                key={entry.id}
                value={entry.id}
                className={(state) => railItem(state.selected)}
              >
                <Icon glyph={entry.glyph} size={14} />
                {entry.label}
              </TabsTab>
            ))}
          </Fragment>
        ))}
      </TabsList>
      {SECTION_IDS.map((id) => (
        <TabsPanel key={id} value={id} className={PANE}>
          {panes[id]}
        </TabsPanel>
      ))}
    </Tabs>
  );
}

function useSectionPanes(
  onOpenTool: (id: string) => void,
  onActivate: (id: number) => void,
): Record<SectionId, ReactNode> {
  const workspace = useWorkspaceContext();
  const placeNode = useNodePlacement();
  const active =
    workspace.selected === null ? null : (workspace.devices.get(workspace.selected) ?? null);
  const target = libraryTarget(workspace.graph, workspace.devices, workspace.selected);

  const openRecording = (recording: RecordingInfo): void => {
    const id = newNodeId("device", nodeIds(workspace.graph));
    const node = recordingNodeFor(recording, id, placeNode(workspace.graph, "device"));
    workspace.edit((snapshot) => ({ ...snapshot, graph: addNode(snapshot.graph, node) }));
    workspace.select(id);
    workspace.apply();
  };

  return {
    templates: (
      <TemplatesPanel
        node={active === null ? null : workspace.selected}
        active={active}
        onApplied={() => workspace.apply()}
      />
    ),
    examples: <ExamplesPanel onImported={onActivate} />,
    presets: <PresetsPanel />,
    bookmarks: <BookmarksPanel target={target} />,
    bands: <BandsPanel target={target} />,
    recordings: <RecordingsPanel onOpen={openRecording} />,
    tools: <ToolsPanel onOpen={onOpenTool} />,
    phones: <PhonesPanel />,
    accounts: <AccountsPanel />,
  };
}
