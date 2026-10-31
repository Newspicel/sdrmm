import { isValidElement } from "react";
import { describe, expect, it } from "vitest";
import { RadiosondeView } from "../components/decoders/radiosonde";
import type { SinkGroup } from "../lib/sinks";
import { renderPanel } from "./panels";

const GROUP: SinkGroup = { source: "sonde", kind: "radiosonde", entities: [], records: [] };

describe("renderPanel", () => {
  it("mounts a panel as its own component so its hooks are not the caller's", () => {
    const element = renderPanel({ radiosonde: RadiosondeView }, GROUP);
    expect(isValidElement(element) && element.type).toBe(RadiosondeView);
  });

  it("leaves a kind without a panel to the generic view", () => {
    expect(renderPanel({}, GROUP)).toBeNull();
  });
});
