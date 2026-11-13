import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Secret } from "../../lib/types";
import { SecretChip } from "./SecretChip";

function render(secret: Secret | undefined): string {
  return renderToStaticMarkup(
    <SecretChip label="Token" title="Token" secret={secret} onSet={() => {}} />,
  );
}

describe("SecretChip", () => {
  it("shows a stored credential as set, never its id", () => {
    const html = render({ secret: "id-7f3a" });
    expect(html).not.toContain("id-7f3a");
    expect(html).toContain(">set<");
  });

  it("says none when nothing is kept", () => {
    expect(render("")).toContain(">none<");
    expect(render(undefined)).toContain(">none<");
  });
});
