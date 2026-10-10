import { useState } from "react";
import { type Rgb, rgbToHex } from "../gl/colormap";
import { Button } from "./BaseControls";
import { ColorPicker } from "./ColorPicker";

export function GradientEditor({
  stops,
  onChange,
}: {
  stops: readonly Rgb[];
  onChange: (stops: readonly Rgb[]) => void;
}) {
  const [selected, setSelected] = useState(stops.length - 1);
  const stop = stops[selected];
  const last = Math.max(1, stops.length - 1);
  const marks = stops.map((colour, index) => ({ colour, index, at: (index / last) * 100 }));

  return (
    <div className="flex flex-col gap-2 px-1.5">
      <div>
        <div
          className="h-4 rounded-[3px] border border-line"
          style={{ background: `linear-gradient(to right, ${stops.map(rgbToHex).join(", ")})` }}
        />
        <div className="relative h-4">
          {marks.map((mark) => (
            <Button
              key={mark.at}
              type="button"
              aria-label={`Colour stop ${mark.index + 1}`}
              aria-pressed={mark.index === selected}
              onClick={() => setSelected(mark.index)}
              className={`absolute top-1 size-3 -translate-x-1/2 rotate-45 rounded-[2px] border-2 transition-colors duration-100 ${
                mark.index === selected
                  ? "border-accent"
                  : "border-line-strong hover:border-ink-dim"
              }`}
              style={{ left: `${mark.at}%`, background: rgbToHex(mark.colour) }}
            />
          ))}
        </div>
      </div>
      {stop !== undefined && (
        <ColorPicker
          key={selected}
          label={`Colour stop ${selected + 1}`}
          value={stop}
          onChange={(rgb) => onChange(stops.with(selected, rgb))}
        />
      )}
    </div>
  );
}
