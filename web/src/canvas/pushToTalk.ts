import { useEffect, useState } from "react";
import type { PatchGraph } from "../lib/types";
import { isTyping } from "./useHotkeys";

export const TALK_KEY = " ";

const OWN_SPACE = "button, a, select, summary, [role='button'], [role='checkbox'], [role='switch']";

export type TalkPress = Pick<KeyboardEvent, "key" | "repeat" | "ctrlKey" | "metaKey" | "altKey">;

/**
 * Whether a key press starts talking: the bare key, once, and not while `taken` by typing or
 * by a control that uses the key itself.
 */
export function startsTalking(event: TalkPress, taken: boolean): boolean {
  const bare = !(event.repeat || event.ctrlKey || event.metaKey || event.altKey);
  return event.key === TALK_KEY && bare && !taken;
}

function takesKey(target: EventTarget | null): boolean {
  return isTyping(target) || (target instanceof Element && target.closest(OWN_SPACE) !== null);
}

/** The microphones the key keys: the selected one, or all of them when none is selected. */
export function talkTargets(graph: PatchGraph, selected: string | null): string[] {
  const microphones = graph.nodes.filter((node) => node.kind === "microphone");
  const chosen = microphones.filter((node) => node.id === selected);
  return (chosen.length > 0 ? chosen : microphones).map((node) => node.id);
}

/** True while the talk key is held; a lost focus lets go of it. */
export function usePushToTalk(): boolean {
  const [held, setHeld] = useState(false);
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (startsTalking(event, takesKey(event.target))) {
        setHeld(true);
        event.preventDefault();
      }
    };
    const onKeyUp = (event: KeyboardEvent) => {
      if (event.key === TALK_KEY) {
        setHeld(false);
      }
    };
    const release = () => setHeld(false);
    document.addEventListener("keydown", onKeyDown);
    document.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", release);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", release);
    };
  }, []);
  return held;
}
