import type { Options } from "../../../components/controls";
import {
  RELATIVE_MARKS,
  type RoseMark,
  type RoseNeedle,
  type RoseWedge,
  TRUE_MARKS,
} from "../../../components/Rose";
import type { BearingView } from "../../../lib/types";

export const NEEDS_HEADING = "Needs heading";

export type BearingFrame = "relative" | "true";

export function frameOptions(trueAvailable: boolean): Options<BearingFrame> {
  return [
    { value: "relative", label: "Rel", title: "Relative to the array" },
    {
      value: "true",
      label: "True",
      title: trueAvailable ? "From true north" : NEEDS_HEADING,
      disabled: !trueAvailable,
    },
  ];
}

export function shownFrame(picked: BearingFrame | null, trueAvailable: boolean): BearingFrame {
  if (!trueAvailable) {
    return "relative";
  }
  return picked ?? "true";
}

export function roseLetters(trueFrame: boolean): readonly RoseMark[] {
  return trueFrame ? TRUE_MARKS : RELATIVE_MARKS;
}

export function frameRotation(view: BearingView | null, frame: BearingFrame): number {
  return frame === "true" ? (view?.heading_deg ?? 0) : 0;
}

type Needle = BearingView["needles"][number];

export function needleDeg(needle: Needle, frame: BearingFrame): number | null {
  return frame === "true" ? (needle.true_deg ?? null) : needle.relative_deg;
}

export function roseNeedles(view: BearingView | null, frame: BearingFrame): RoseNeedle[] {
  return (view?.needles ?? []).flatMap((needle) => {
    const deg = needleDeg(needle, frame);
    return deg === null ? [] : [{ deg, weight: needle.weight }];
  });
}

export function roseWedge(view: BearingView | null, frame: BearingFrame): RoseWedge | null {
  const first = view?.needles[0];
  const deg = first === undefined ? null : needleDeg(first, frame);
  const sigmaDeg = view?.sigma_deg;
  return deg === null || sigmaDeg == null ? null : { deg, sigmaDeg };
}
