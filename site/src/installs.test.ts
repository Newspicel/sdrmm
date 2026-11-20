import { describe, expect, it } from "vitest";
import { FALLBACK_VERSION, INSTALLS, installFor, withVersion } from "./installs";

describe("installFor", () => {
  it("picks the package manager of the platform", () => {
    expect(installFor("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")).toBe("container");
    expect(installFor("Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5)")).toBe("brew");
    expect(installFor("Mozilla/5.0 (X11; Linux x86_64)")).toBe("apt");
    expect(installFor("Mozilla/5.0 (X11; Fedora; Linux x86_64)")).toBe("dnf");
  });

  it("falls back to Homebrew on phones", () => {
    expect(installFor("Mozilla/5.0 (Linux; Android 15; Pixel 9)")).toBe("brew");
  });

  it("only names installs that exist", () => {
    const ids = new Set(INSTALLS.map((install) => install.id));
    for (const agent of ["Windows", "Macintosh", "X11", "Fedora", "Android"]) {
      expect(ids.has(installFor(agent))).toBe(true);
    }
  });
});

describe("INSTALLS", () => {
  it("keeps shell line continuations", () => {
    const apt = INSTALLS.find((install) => install.id === "apt");
    expect(apt?.shells[0]?.lines).toContain("key.gpg \\\n");
  });

  it("splits the Homebrew app and server into separate boxes", () => {
    const brew = INSTALLS.find((install) => install.id === "brew");
    expect(brew?.shells.map((shell) => shell.label)).toEqual(["App", "Server"]);
  });

  it("updates the Nix version line cleanly", () => {
    const nix = INSTALLS.find((install) => install.id === "nix");
    expect(nix?.shells[0]?.dynamicVersion).toBe(true);
    expect(nix?.shells[0]?.lines).toContain(`VERSION=${FALLBACK_VERSION}`);
    expect(withVersion(nix?.shells[0]?.lines ?? "", "2.5.0")).toContain("VERSION=2.5.0");
  });
});
