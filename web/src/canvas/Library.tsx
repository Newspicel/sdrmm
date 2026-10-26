import { Tabs } from "@base-ui/react/tabs";
import { BandsPanel } from "../components/BandsPanel";
import { BookmarksPanel } from "../components/BookmarksPanel";
import { TAB_BAR, tab } from "../components/controls";
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
  { id: "presets", label: "Presets" },
  { id: "bookmarks", label: "Bookmarks" },
  { id: "bands", label: "Bands" },
  { id: "occupancy", label: "Occupancy" },
  { id: "recordings", label: "Recordings" },
  { id: "tools", label: "Tools" },
  { id: "phones", label: "Phones" },
] as const;

export function Library({ onOpenTool }: { onOpenTool: (id: string) => void }) {
  const workspace = useWorkspaceContext();
  const placeNode = useNodePlacement();
  const active =
    workspace.selected === null ? null : (workspace.devices.get(workspace.selected) ?? null);
  const target = libraryTarget(workspace.graph, workspace.devices, workspace.selected);

  const openRecording = (recording: RecordingInfo): void => {
    const id = newNodeId("recording", nodeIds(workspace.graph));
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: addNode(
        snapshot.graph,
        recordingNodeFor(recording, id, placeNode(snapshot.graph, "recording")),
      ),
    }));
    workspace.select(id);
    workspace.apply();
  };

  return (
    <Tabs.Root defaultValue="templates" className="flex flex-col overflow-hidden rounded-control">
      <Tabs.List className={`${TAB_BAR} shrink-0 px-1`} aria-label="Library section">
        {TABS.map((entry) => (
          <Tabs.Tab key={entry.id} value={entry.id} className={(state) => tab(state.active)}>
            {entry.label}
          </Tabs.Tab>
        ))}
      </Tabs.List>
      <Tabs.Panel value="templates" className="max-h-[28rem] overflow-y-auto">
        <TemplatesPanel
          node={active === null ? null : workspace.selected}
          active={active}
          onApplied={() => workspace.apply()}
        />
      </Tabs.Panel>
      <Tabs.Panel value="presets" className="max-h-[28rem] overflow-y-auto">
        <PresetsPanel />
      </Tabs.Panel>
      <Tabs.Panel value="bookmarks" className="max-h-[28rem] overflow-y-auto">
        <BookmarksPanel target={target} />
      </Tabs.Panel>
      <Tabs.Panel value="bands" className="max-h-[28rem] overflow-y-auto">
        <BandsPanel target={target} />
      </Tabs.Panel>
      <Tabs.Panel value="occupancy" className="max-h-[28rem] overflow-y-auto">
        <OccupancyPanel target={target} />
      </Tabs.Panel>
      <Tabs.Panel value="recordings" className="max-h-[28rem] overflow-y-auto">
        <RecordingsPanel onOpen={openRecording} />
      </Tabs.Panel>
      <Tabs.Panel value="tools" className="max-h-[28rem] overflow-y-auto">
        <ToolsPanel onOpen={onOpenTool} />
      </Tabs.Panel>
      <Tabs.Panel value="phones" className="max-h-[28rem] overflow-y-auto">
        <PhonesPanel />
      </Tabs.Panel>
    </Tabs.Root>
  );
}
