import { Tabs, TabsList, TabsPanel, TabsTab } from "@ui-sdk/react/tabs";
import { BandsPanel } from "../components/BandsPanel";
import { BookmarksPanel } from "../components/BookmarksPanel";
import { TAB_BAR, tab } from "../components/controls";
import { ExamplesPanel } from "../components/ExamplesPanel";
import { OccupancyPanel } from "../components/OccupancyPanel";
import { PresetsPanel } from "../components/PresetsPanel";
import { RecordingsPanel } from "../components/RecordingsPanel";
import { TemplatesPanel } from "../components/TemplatesPanel";
import type { RecordingInfo } from "../lib/types";
import { ToolsPanel } from "../tools/ToolsPanel";
import { useWorkspaceContext } from "./context";
import { addNode, newNodeId, nodeIds } from "./graph";
import { libraryTarget } from "./libraryTarget";
import { recordingNodeFor } from "./nodes/recordingNode";
import { PhonesPanel } from "./PhonesPanel";
import { useNodePlacement } from "./placement";

const TABS = [
  { id: "templates", label: "Templates" },
  { id: "examples", label: "Examples" },
  { id: "presets", label: "Presets" },
  { id: "bookmarks", label: "Bookmarks" },
  { id: "bands", label: "Bands" },
  { id: "occupancy", label: "Occupancy" },
  { id: "recordings", label: "Recordings" },
  { id: "tools", label: "Tools" },
  { id: "phones", label: "Phones" },
] as const;

export function Library({
  onOpenTool,
  onActivate,
}: {
  onOpenTool: (id: string) => void;
  onActivate: (id: number) => void;
}) {
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

  return (
    <Tabs defaultValue="templates" className="flex flex-col overflow-hidden rounded-control">
      <TabsList className={`${TAB_BAR} shrink-0 px-1`} aria-label="Library section">
        {TABS.map((entry) => (
          <TabsTab key={entry.id} value={entry.id} className={(state) => tab(state.selected)}>
            {entry.label}
          </TabsTab>
        ))}
      </TabsList>
      <TabsPanel value="templates" className="max-h-[28rem] overflow-y-auto">
        <TemplatesPanel
          node={active === null ? null : workspace.selected}
          active={active}
          onApplied={() => workspace.apply()}
        />
      </TabsPanel>
      <TabsPanel value="examples" className="max-h-[28rem] overflow-y-auto">
        <ExamplesPanel onImported={onActivate} />
      </TabsPanel>
      <TabsPanel value="presets" className="max-h-[28rem] overflow-y-auto">
        <PresetsPanel />
      </TabsPanel>
      <TabsPanel value="bookmarks" className="max-h-[28rem] overflow-y-auto">
        <BookmarksPanel target={target} />
      </TabsPanel>
      <TabsPanel value="bands" className="max-h-[28rem] overflow-y-auto">
        <BandsPanel target={target} />
      </TabsPanel>
      <TabsPanel value="occupancy" className="max-h-[28rem] overflow-y-auto">
        <OccupancyPanel target={target} />
      </TabsPanel>
      <TabsPanel value="recordings" className="max-h-[28rem] overflow-y-auto">
        <RecordingsPanel onOpen={openRecording} />
      </TabsPanel>
      <TabsPanel value="tools" className="max-h-[28rem] overflow-y-auto">
        <ToolsPanel onOpen={onOpenTool} />
      </TabsPanel>
      <TabsPanel value="phones" className="max-h-[28rem] overflow-y-auto">
        <PhonesPanel />
      </TabsPanel>
    </Tabs>
  );
}
