import type { ReactNode } from "react";
import { Chips } from "../../components/face/Chips";
import { FoldSection } from "../../components/face/Fold";
import { Readout, Readouts } from "../../components/face/Readouts";
import { SettingsForm } from "../../components/SettingsForm";
import { clearAction, useRefusalStore } from "../../lib/refusals";
import { type Form, type Settings, sections } from "../../lib/settingsForm";
import type { Field, ParamChoice, PatchCatalog, PatchNode } from "../../lib/types";
import { formatValue } from "../../sinks/values";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { defaultBody, processorDefaults, processorTypeOf } from "../newNode";
import { FaceEmpty } from "./NodeShell";
import { NO_CATALOG, SETTINGS_ACTION } from "./processorFace";

const NO_FORM: Form = [];
const NO_CHOICES: readonly ParamChoice[] = [];

export function formOf(catalog: PatchCatalog, node: PatchNode): Form {
  if (node.kind === "processor") {
    return processorTypeOf(catalog, node.data?.processor_type ?? "")?.settings ?? NO_FORM;
  }
  return catalog.nodes.find((entry) => entry.kind === node.kind)?.settings ?? NO_FORM;
}

function storedSettings(body: unknown): Settings | null {
  const data = (body as { data?: { settings?: unknown } } | null)?.data;
  const settings = data?.settings;
  return typeof settings === "object" && settings !== null ? (settings as Settings) : null;
}

/** The node's settings, or what a fresh node of its kind starts on. */
export function nodeSettings(catalog: PatchCatalog, node: PatchNode): Settings | null {
  if (node.kind === "processor") {
    const type = processorTypeOf(catalog, node.data?.processor_type ?? "");
    return storedSettings(node) ?? (type === undefined ? null : processorDefaults(type));
  }
  return storedSettings(node) ?? storedSettings(defaultBody(catalog, node.kind));
}

function withSettings(node: PatchNode, settings: Settings): PatchNode {
  const data = (node as { data?: object }).data ?? {};
  return { ...node, data: { ...data, settings } } as PatchNode;
}

export function useSettingsCommit(node: PatchNode): (settings: Settings) => void {
  const workspace = useWorkspaceContext();
  return (settings) => {
    if (nodeSettings(workspace.context.catalog, node) === null) {
      useRefusalStore.getState().flag(node.id, NO_CATALOG, "action", SETTINGS_ACTION);
      return;
    }
    clearAction(node.id, SETTINGS_ACTION);
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (stored) =>
        stored.kind === node.kind ? withSettings(stored, settings) : stored,
      ),
    }));
    workspace.apply();
  };
}

/**
 * A node's settings as the catalog's form draws them: loose chips after `lead`, the face's own
 * view controls, then one fold per section.
 */
export function NodeForm({
  node,
  lanes = 0,
  choices = NO_CHOICES,
  lead,
}: {
  node: PatchNode;
  lanes?: number;
  choices?: readonly ParamChoice[];
  lead?: ReactNode;
}) {
  const workspace = useWorkspaceContext();
  const commit = useSettingsCommit(node);
  const catalog = workspace.context.catalog;
  const form = formOf(catalog, node);
  const settings = nodeSettings(catalog, node);
  if (settings === null) {
    return <FaceEmpty hint={NO_CATALOG} />;
  }
  if (form.length === 0) {
    return null;
  }
  const shared = { form, settings, choices, lanes, onSettings: commit };
  return (
    <>
      <Chips className="shrink-0">
        {lead}
        <SettingsForm {...shared} section={null} />
      </Chips>
      {sections(form).map((section) => (
        <FoldSection key={section} label={section}>
          <Chips inset="none">
            <SettingsForm {...shared} section={section} />
          </Chips>
        </FoldSection>
      ))}
    </>
  );
}

/** Readouts a node's status publishes, then how long ago it arrived. */
export function StatusGrid({
  readouts,
  age,
  columns = 2,
}: {
  readouts: readonly Field[];
  age?: string;
  columns?: 1 | 2 | 4;
}) {
  return (
    <Readouts columns={columns}>
      {readouts.map((field) => (
        <Readout key={field.key} label={field.label}>
          {formatValue(field.value)}
        </Readout>
      ))}
      {age !== undefined && <Readout label="Age">{age}</Readout>}
    </Readouts>
  );
}
