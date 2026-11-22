import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { Gate } from "./AccessGate";

function gate(token_required: boolean, sign_in: boolean, notice?: string): string {
  return renderToStaticMarkup(
    <Gate
      auth={{ token_required, sign_in }}
      notice={notice}
      signInHref="/api/auth/login?next=%2F"
      onToken={() => undefined}
    />,
  );
}

describe("Gate", () => {
  it("offers sign-in alone when the server has no token", () => {
    const html = gate(false, true);
    expect(html).toContain('href="/api/auth/login?next=%2F"');
    expect(html).not.toContain("Shared token");
  });

  it("offers the token alone when sign-in is off", () => {
    const html = gate(true, false);
    expect(html).toContain('aria-label="Shared token"');
    expect(html).not.toContain("Sign in");
  });

  it("offers both and says why the last try failed", () => {
    const html = gate(true, true, "This account has no access.");
    expect(html).toContain("Sign in");
    expect(html).toContain("Shared token");
    expect(html).toContain('role="alert"');
    expect(html).toContain("This account has no access.");
  });
});
