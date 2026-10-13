import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { MHZ_DIGITS } from "./format";
import { NumberField } from "./NumberField";

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
});
