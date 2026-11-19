import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { Slider } from "./Slider";

describe("Slider", () => {
  const html = renderToStaticMarkup(
    <Slider label="Gain" value={30} min={0} max={100} onChange={() => {}} />,
  );

  it("names the range input the thumb holds", () => {
    expect(html).toMatch(/<input aria-label="Gain"[^>]*role="slider"[^>]*value="30"/);
  });

  it("keeps the thumb inside the rail at both ends", () => {
    expect(html).toMatch(/class="mx-0\.5 h-full" data-slot="slider-track"/);
    expect(html).toMatch(/class="-ml-0\.5 box-content[^"]*" data-slot="slider-indicator"/);
  });

  it("marks a disabled slider", () => {
    const off = renderToStaticMarkup(
      <Slider label="Gain" value={30} min={0} max={100} disabled onChange={() => {}} />,
    );
    expect(off).toMatch(/data-slot="slider-control"[^>]*data-disabled=""/);
  });
});
