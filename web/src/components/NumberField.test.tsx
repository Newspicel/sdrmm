import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { MHZ_DIGITS } from "./format";
import { arrowStep, NumberField, stepped } from "./NumberField";

describe("NumberField", () => {
  it("keeps typed precision finer than the step", () => {
    const html = renderToStaticMarkup(
      <NumberField
        label="Start"
        value={390.19}
        step={0.1}
        digits={MHZ_DIGITS}
        onCommit={() => {}}
      />,
    );
    expect(html).toContain('value="390.19"');
  });

  it("rounds to the step without digits", () => {
    const html = renderToStaticMarkup(
      <NumberField label="Start" value={390.19} step={0.1} onCommit={() => {}} />,
    );
    expect(html).toContain('value="390.2"');
  });

  it("steps arrows by the field's step, not the typing grid", () => {
    const key = { key: "ArrowUp", shiftKey: false, altKey: false };
    expect(arrowStep(key, 0.1)).toBe(0.1);
    expect(arrowStep({ ...key, key: "ArrowDown" }, 0.1)).toBe(-0.1);
    expect(arrowStep({ ...key, shiftKey: true }, 0.1)).toBe(10);
    expect(arrowStep({ ...key, altKey: true }, 0.1)).toBe(0.1);
    expect(arrowStep(key, undefined)).toBe(1);
    expect(arrowStep({ ...key, key: "Enter" }, 0.1)).toBeNull();
  });

  it("keeps typed precision when stepping and stays in bounds", () => {
    expect(stepped(390.19, 0.1, { digits: 6 })).toBe(390.29);
    expect(stepped(1765.9, 1, { max: 1766, digits: 6 })).toBe(1766);
    expect(stepped(0.5, -1, { min: 0, digits: 1 })).toBe(0);
    expect(stepped(null, 1, { digits: 0 })).toBe(1);
  });
});
