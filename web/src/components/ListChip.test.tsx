import type { FunctionComponent, ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Form, Settings } from "../lib/settingsForm";
import type { PresetOption, SettingField } from "../lib/types";
import { fitIds, ItemControl } from "./ListChip";

type Props = Record<string, unknown>;

function field(key: string, control: SettingField["control"], label = key): SettingField {
  return { key, label, control };
}

function entryControl(item: Form, entry: Settings, key: string) {
  const edits: Settings[] = [];
  const target = item.find((one) => one.key === key);
  if (target === undefined) {
    throw new Error(`no field ${key}`);
  }
  const element = ItemControl({
    item,
    entry,
    field: target,
    name: `Row ${key}`,
    onEdit: (next) => edits.push(next),
  }) as ReactElement<Props>;
  return { element, edits };
}

function expand(element: ReactElement<Props>): ReactElement<Props> {
  return (element.type as FunctionComponent<Props>)(element.props) as ReactElement<Props>;
}

const SLOW: PresetOption = { label: "Slow", values: { "timing.rate": 12, mode: "a" } };
const FAST: PresetOption = { label: "Fast", values: { "timing.rate": 48, mode: "b" } };

const ITEM: Form = [
  field("name", { type: "text" }),
  field("preset", { type: "preset", options: [SLOW, FAST] }),
  field("timing.rate", { type: "number" }),
  field("mode", { type: "text" }),
  field("hops", { type: "list", item: [field("hz", { type: "number", unit: "Hz" })] }),
  field("freq", { type: "number", unit: "MHz", scale: 1e6, min: 1e6, step: 1e3 }),
];

describe("list entry controls", () => {
  it("picks a nested preset across dotted keys", () => {
    const entry = { name: "a", timing: { rate: 12 }, mode: "a" };
    const { element, edits } = entryControl(ITEM, entry, "preset");
    expect(element.props.chosen).toBe(0);
    (element.props.onPick as (option: PresetOption) => void)(FAST);
    expect(edits).toEqual([{ name: "a", timing: { rate: 48 }, mode: "b" }]);
    expect(entryControl(ITEM, { timing: { rate: 9 } }, "preset").element.props.chosen).toBe(-1);
  });

  it("names a custom entry in the preset select", () => {
    const html = renderToStaticMarkup(
      entryControl(ITEM, { timing: { rate: 9 } }, "preset").element,
    );
    expect(html).toContain("Custom");
  });

  it("edits a list nested inside an entry", () => {
    const entry = { name: "a", hops: [{ hz: 1 }, { hz: 2 }] };
    const { element, edits } = entryControl(ITEM, entry, "hops");
    expect(element.props.entries).toEqual([{ hz: 1 }, { hz: 2 }]);
    expect(renderToStaticMarkup(element)).toContain(">2<");
    (element.props.onChange as (entries: Settings[]) => void)([{ hz: 3 }]);
    expect(edits).toEqual([{ name: "a", hops: [{ hz: 3 }] }]);
  });

  it("shows a scaled number and stores it unscaled", () => {
    const { element, edits } = entryControl(ITEM, { freq: 145_500_000 }, "freq");
    const number = expand(element);
    expect(number.props.value).toBe(145.5);
    expect((number.props.spec as { min: number; step: number }).min).toBe(1);
    expect((number.props.spec as { min: number; step: number }).step).toBe(0.001);
    (number.props.onCommit as (shown: number) => void)(144.8);
    expect(edits).toEqual([{ freq: 144_800_000 }]);
  });
});

describe("entry ids", () => {
  it("keeps ids it has and grows or cuts to the count", () => {
    const grown = fitIds(["a"], 3);
    expect(grown[0]).toBe("a");
    expect(new Set(grown).size).toBe(3);
    expect(fitIds(["a", "b", "c"], 2)).toEqual(["a", "b"]);
  });
});
