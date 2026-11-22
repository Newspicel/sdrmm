import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { RoleContext } from "../lib/role";
import type { UserRole } from "../lib/types";
import { ViewOnly } from "./ViewOnly";

function view(role: UserRole): string {
  return renderToStaticMarkup(
    <RoleContext value={role}>
      <ViewOnly>
        <input aria-label="Gain" />
      </ViewOnly>
    </RoleContext>,
  );
}

describe("ViewOnly", () => {
  it("disables every control for a viewer", () => {
    expect(view("viewer")).toMatch(/<fieldset disabled=""/);
  });

  it("leaves controls live for an operator and an admin", () => {
    expect(view("operator")).not.toContain("disabled");
    expect(view("admin")).not.toContain("disabled");
  });
});
