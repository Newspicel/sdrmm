import { Slider } from "@base-ui/react/slider";
import {
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  useState,
} from "react";
import { hexToRgb, type Rgb, rgbToHex } from "../gl/colormap";
import { Input } from "./BaseControls";
import { FIELD } from "./controls";
import { type Hsv, hsvToRgb, rgbToHsv } from "./hsv";

const KEY_STEP = 0.02;
const KEY_STEP_LARGE = 0.1;
const HUE_TRACK =
  "linear-gradient(to right, #f00 0%, #ff0 17%, #0f0 33%, #0ff 50%, #00f 67%, #f0f 83%, #f00 100%)";

export function ColorPicker({
  label,
  value,
  onChange,
}: {
  label: string;
  value: Rgb;
  onChange: (rgb: Rgb) => void;
}) {
  const [hsv, setHsv] = useState<Hsv>(() => rgbToHsv(value));
  const [draft, setDraft] = useState<string | null>(null);

  const apply = (next: Hsv): void => {
    setHsv(next);
    onChange(hsvToRgb(next));
  };

  const typeHex = (text: string): void => {
    setDraft(text);
    const rgb = hexToRgb(text.startsWith("#") ? text : `#${text}`);
    if (rgb !== null) {
      setHsv(rgbToHsv(rgb));
      onChange(rgb);
    }
  };

  return (
    <div className="flex flex-col gap-2">
      <SaturationValue label={label} hsv={hsv} onChange={apply} />
      <HueSlider label={`${label} hue`} hue={hsv.h} onChange={(h) => apply({ ...hsv, h })} />
      <div className="flex items-center gap-2">
        <span
          className="size-6 shrink-0 rounded-[3px] border border-line"
          style={{ background: rgbToHex(value) }}
        />
        <Input
          aria-label={`${label} hex`}
          spellCheck={false}
          className={`${FIELD} w-full font-mono`}
          value={draft ?? rgbToHex(value)}
          onChange={(event) => typeHex(event.target.value)}
          onBlur={() => setDraft(null)}
        />
      </div>
    </div>
  );
}

function SaturationValue({
  label,
  hsv,
  onChange,
}: {
  label: string;
  hsv: Hsv;
  onChange: (hsv: Hsv) => void;
}) {
  const pick = (event: ReactPointerEvent<HTMLDivElement>): void => {
    const rect = event.currentTarget.getBoundingClientRect();
    onChange({
      ...hsv,
      s: unit((event.clientX - rect.left) / rect.width),
      v: unit(1 - (event.clientY - rect.top) / rect.height),
    });
  };

  const nudge = (event: ReactKeyboardEvent<HTMLDivElement>): void => {
    const step = event.shiftKey ? KEY_STEP_LARGE : KEY_STEP;
    const moves: Record<string, Partial<Hsv>> = {
      ArrowLeft: { s: unit(hsv.s - step) },
      ArrowRight: { s: unit(hsv.s + step) },
      ArrowDown: { v: unit(hsv.v - step) },
      ArrowUp: { v: unit(hsv.v + step) },
    };
    const move = moves[event.key];
    if (move !== undefined) {
      event.preventDefault();
      onChange({ ...hsv, ...move });
    }
  };

  return (
    <div
      role="slider"
      tabIndex={0}
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(hsv.v * 100)}
      aria-valuetext={`saturation ${Math.round(hsv.s * 100)}%, brightness ${Math.round(hsv.v * 100)}%`}
      data-hotkeys="off"
      className="relative h-28 cursor-crosshair touch-none rounded-[3px] outline-none focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-2"
      style={{
        background: `linear-gradient(to top, #000, transparent), linear-gradient(to right, #fff, hsl(${hsv.h} 100% 50%))`,
      }}
      onPointerDown={(event) => {
        event.currentTarget.setPointerCapture(event.pointerId);
        pick(event);
      }}
      onPointerMove={(event) => {
        if (event.currentTarget.hasPointerCapture(event.pointerId)) {
          pick(event);
        }
      }}
      onKeyDown={nudge}
    >
      <span
        className="pointer-events-none absolute size-3 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white shadow-raised"
        style={{
          left: `${hsv.s * 100}%`,
          top: `${(1 - hsv.v) * 100}%`,
          background: rgbToHex(hsvToRgb(hsv)),
        }}
      />
    </div>
  );
}

function HueSlider({
  label,
  hue,
  onChange,
}: {
  label: string;
  hue: number;
  onChange: (hue: number) => void;
}) {
  return (
    <Slider.Root
      data-hotkeys="off"
      thumbAlignment="edge"
      className="flex w-full"
      value={hue}
      min={0}
      max={359}
      step={1}
      onValueChange={(next) => {
        if (typeof next === "number") {
          onChange(next);
        }
      }}
    >
      <Slider.Control className="flex h-5 w-full cursor-pointer touch-none items-center">
        <Slider.Track className="h-2.5 w-full rounded-full" style={{ background: HUE_TRACK }}>
          <Slider.Thumb
            aria-label={label}
            className="size-3.5 rounded-full border-2 border-white shadow-raised has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-accent has-[:focus-visible]:outline-offset-2"
            style={{ background: `hsl(${hue} 100% 50%)` }}
          />
        </Slider.Track>
      </Slider.Control>
    </Slider.Root>
  );
}

function unit(x: number): number {
  return Math.min(1, Math.max(0, x));
}
