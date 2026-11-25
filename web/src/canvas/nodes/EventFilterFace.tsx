import { Chips, ChoiceChip, NumberChip, ToggleChip } from "../../components/face/Chips";
import { Readout, Readouts } from "../../components/face/Readouts";
import { TextChip } from "../../components/face/TextChip";
import { isEventKind, kindLabel } from "../../lib/eventKinds";
import { MAX_FILTER_DURATION_MS, MAX_FILTER_TEXT_LEN } from "../../lib/limits";
import { useFeeds } from "../../lib/resolved";
import type { EventFilterNode, EventKindEntry, PatchNode, PatchNodeOf } from "../../lib/types";
import { useWorkspaceNow, useWorkspaceSelect } from "../context";
import { patchNode } from "../graph";
import { Inspect, InspectSection } from "../inspector";
import {
  FILTER_MODES,
  filterMode,
  filterModeLabel,
  formatIds,
  formatWords,
  fromTriState,
  offeredBy,
  type PredicateKey,
  parseIds,
  parseWords,
  ruleCount,
  sectionsFor,
  stationLabel,
  type TriState,
  toTriState,
} from "./eventFilter";
import { FaceBody, NodeShell } from "./NodeShell";

type TriKey = "has_position" | "encrypted" | "emergency";

const TRI_STATES = [
  { value: "any", label: "Any" },
  { value: "yes", label: "Yes" },
  { value: "no", label: "No" },
] as const;

export function EventFilterFace({ node }: { node: PatchNode }) {
  if (node.kind !== "event_filter") {
    return null;
  }
  return <Face node={node} />;
}

function Face({ node }: { node: PatchNodeOf<"event_filter"> }) {
  const now = useWorkspaceNow();
  const settings: EventFilterNode = node.data ?? {};
  const sources = useFeeds(node.id, "events");
  const entries = useWorkspaceSelect((workspace) => workspace.context.catalog.event_kinds);
  const offered = offeredBy(sources).filter((kind) => isEventKind(kind, entries));
  const kinds = settings.kinds ?? [];
  const narrowed = kinds.length > 0 ? kinds : offered;
  const sections = sectionsFor(narrowed, entries);

  const edit = (next: Partial<EventFilterNode>) => {
    now().edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        current.kind === "event_filter"
          ? { ...current, data: { ...current.data, ...next } }
          : current,
      ),
    }));
  };

  return (
    <NodeShell node={node} title="Event filter" category="tool">
      <FaceBody>
        <Readouts>
          <Readout label="Mode" title="Keep passes only what matches; Drop removes what matches">
            {filterModeLabel(settings)}
          </Readout>
          <Readout label="Kinds" title="Event kinds the rules look at">
            {kinds.length === 0 ? "all" : kinds.map((kind) => kindLabel(kind, entries)).join(", ")}
          </Readout>
          <Readout label="Rules" title="Rules set in the settings">
            {ruleCount(settings)}
          </Readout>
        </Readouts>
      </FaceBody>
      <Inspect node={node.id}>
        <InspectSection>
          <Chips inset="none">
            <ChoiceChip
              label="Mode"
              title="Keep passes only what matches; Drop removes what matches"
              value={filterMode(settings)}
              options={FILTER_MODES}
              onChange={(mode) => edit({ mode })}
            />
            {offered.length > 1 &&
              offered.map((kind) => (
                <ToggleChip
                  key={kind}
                  label={kindLabel(kind, entries)}
                  title={`Match ${kindLabel(kind, entries)} events`}
                  on={kinds.includes(kind)}
                  onChange={(on) =>
                    edit({
                      kinds: on
                        ? [...kinds, kind].toSorted()
                        : kinds.filter((held) => held !== kind),
                    })
                  }
                />
              ))}
            {sections.flatMap((section) =>
              section.predicates.map((predicate) => (
                <Predicate
                  key={predicate}
                  which={predicate}
                  applies={section.applies}
                  settings={settings}
                  kinds={narrowed}
                  entries={entries}
                  edit={edit}
                />
              )),
            )}
          </Chips>
        </InspectSection>
      </Inspect>
    </NodeShell>
  );
}

function appliesTo(title: string, applies: readonly string[]): string {
  return applies.length === 0 ? title : `${title}. Applies to ${applies.join(", ")}`;
}

function Predicate({
  which,
  applies,
  settings,
  kinds,
  entries,
  edit,
}: {
  which: PredicateKey;
  applies: readonly string[];
  settings: EventFilterNode;
  kinds: readonly string[];
  entries: readonly EventKindEntry[];
  edit: (next: Partial<EventFilterNode>) => void;
}) {
  const tri = (label: string, value: boolean | null | undefined, key: TriKey) => (
    <ChoiceChip
      label={label}
      title={appliesTo(label, applies)}
      value={toTriState(value)}
      options={TRI_STATES}
      quiet={value == null}
      onChange={(next: TriState) => edit({ [key]: fromTriState(next) ?? null })}
    />
  );
  switch (which) {
    case "stations": {
      const label = stationLabel(kinds, entries);
      return (
        <TextChip
          label={label}
          title={appliesTo(label, applies)}
          value={formatWords(settings.stations)}
          placeholder="any"
          onCommit={(text) => edit({ stations: parseWords(text) })}
        />
      );
    }
    case "contains":
      return (
        <TextChip
          label="Contains"
          title={appliesTo("Contains", applies)}
          value={settings.contains ?? ""}
          placeholder="any"
          maxLength={MAX_FILTER_TEXT_LEN}
          onCommit={(text) => edit({ contains: text.trim() || null })}
        />
      );
    case "has_position":
      return tri("Has position", settings.has_position, "has_position");
    case "talkgroups":
      return (
        <TextChip
          label="Talkgroups"
          title={appliesTo("Talkgroups", applies)}
          value={formatIds(settings.talkgroups)}
          placeholder="any"
          onCommit={(text) => edit({ talkgroups: parseIds(text) })}
        />
      );
    case "radios":
      return (
        <TextChip
          label="Radios"
          title={appliesTo("Radios", applies)}
          value={formatIds(settings.radios)}
          placeholder="any"
          onCommit={(text) => edit({ radios: parseIds(text) })}
        />
      );
    case "encrypted":
      return tri("Encrypted", settings.encrypted, "encrypted");
    case "emergency":
      return tri("Emergency", settings.emergency, "emergency");
    case "min_duration_ms":
      return (
        <NumberChip
          label="Longer than"
          title={appliesTo("Longer than", applies)}
          value={(settings.min_duration_ms ?? 0) / 1000}
          unit="s"
          min={0}
          max={MAX_FILTER_DURATION_MS / 1000}
          step={0.5}
          quiet={(settings.min_duration_ms ?? 0) === 0}
          onCommit={(next) => edit({ min_duration_ms: Math.round(next * 1000) })}
        />
      );
  }
}
