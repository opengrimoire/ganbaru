import { describe, expect, it } from "vitest";
import { installDocsLabel, situationNeedsInstall, unconfiguredFamilySituation } from "./provider-install-steps";

describe("provider install steps", () => {
  it("tells a family the device does not search for apart from one it searched without a match", () => {
    expect(unconfiguredFamilySituation(undefined)).toEqual({ kind: "unknown" });
    expect(unconfiguredFamilySituation([])).toEqual({ kind: "not-installed" });
  });

  it("reports the first program found for an unconfigured family", () => {
    expect(unconfiguredFamilySituation(["/home/person/.local/bin/cursor-agent", "/usr/bin/cursor"]))
      .toEqual({ kind: "installed", path: "/home/person/.local/bin/cursor-agent" });
  });

  it("asks for an install only when the program is missing or unknown", () => {
    expect(situationNeedsInstall({ kind: "not-installed" })).toBe(true);
    expect(situationNeedsInstall({ kind: "missing-at", path: "/missing/claude" })).toBe(true);
    expect(situationNeedsInstall({ kind: "unknown" })).toBe(true);
    expect(situationNeedsInstall({ kind: "installed", path: "/usr/bin/claude" })).toBe(false);
    expect(situationNeedsInstall({ kind: "signed-out" })).toBe(false);
  });

  it("drops the scheme and trailing slash from a documentation link", () => {
    expect(installDocsLabel("https://opencode.ai/docs/")).toBe("opencode.ai/docs");
    expect(installDocsLabel("https://cursor.com/docs/cli/installation")).toBe("cursor.com/docs/cli/installation");
  });
});
