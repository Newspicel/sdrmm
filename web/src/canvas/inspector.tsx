import {
  createContext,
  type ReactNode,
  type SyntheticEvent,
  useContext,
  useMemo,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { ChipRows } from "../components/face/Chips";
import { PortalContainerProvider } from "../components/PortalContainer";
import { useWorkspaceContext } from "./context";

interface Slots {
  head: HTMLElement | null;
  body: HTMLElement | null;
}

/** Where `Inspect` draws: the open panel's slots, in place (tests), or nowhere. */
type Target = Slots | "inline";

const NO_SLOTS: Slots = { head: null, body: null };

const InspectorTarget = createContext<Target>(NO_SLOTS);
interface SlotSetter {
  setHead: (element: HTMLElement | null) => void;
  setBody: (element: HTMLElement | null) => void;
}

const SlotSetters = createContext<SlotSetter | null>(null);

export function useSlotSetters(): SlotSetter | null {
  return useContext(SlotSetters);
}
const InspectSource = createContext(true);

export function InspectorProvider({ children }: { children: ReactNode }) {
  const [head, setHead] = useState<HTMLElement | null>(null);
  const [body, setBody] = useState<HTMLElement | null>(null);
  const [setters] = useState<SlotSetter>(() => ({ setHead, setBody }));
  return (
    <SlotSetters value={setters}>
      <InspectorTarget value={{ head, body }}>{children}</InspectorTarget>
    </SlotSetters>
  );
}

/** Draws inspector content in place, so a face renders whole without the panel. */
export function InlineInspector({ children }: { children: ReactNode }) {
  return <InspectorTarget value="inline">{children}</InspectorTarget>;
}

/** A second face of the same node, which leaves the panel to the first. */
export function NoInspect({ children }: { children: ReactNode }) {
  return <InspectSource value={false}>{children}</InspectSource>;
}

const stop = (event: SyntheticEvent) => event.stopPropagation();

/**
 * What a face shows in the inspector while its node is the inspected one. Events stop at the
 * panel, so a click there never reaches the node on the canvas.
 */
export function Inspect({
  node,
  slot = "body",
  children,
}: {
  node: string;
  slot?: keyof Slots;
  children: ReactNode;
}) {
  const workspace = useWorkspaceContext();
  const target = useContext(InspectorTarget);
  const source = useContext(InspectSource);
  const element = target === "inline" ? null : target[slot];
  const popups = useMemo(() => ({ current: element }), [element]);
  if (!source || workspace.inspected !== node) {
    return null;
  }
  if (target === "inline") {
    return slot === "body" ? <ChipRows>{children}</ChipRows> : null;
  }
  if (element === null) {
    return null;
  }
  return createPortal(
    <div className="contents" onClick={stop} onDoubleClick={stop} onContextMenu={stop}>
      <PortalContainerProvider container={popups}>
        <ChipRows>{children}</ChipRows>
      </PortalContainerProvider>
    </div>,
    element,
  );
}

/** A titled group in the inspector. */
export function InspectSection({ label, children }: { label?: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-5 border-t border-line px-4 pt-3 pb-4">
      {label !== undefined && <h3 className="font-semibold text-ink text-xs">{label}</h3>}
      {children}
    </section>
  );
}
