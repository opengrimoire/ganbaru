import { describe, expect, it } from "vitest";
import { providerPermissionFileName, providerSupportsPermissionMode } from "./permission-modes";

describe("Chat permission modes", () => {
  it("exposes manual approval and Full access for every provider transport", () => {
    for (const family of ["codex", "claude", "cursor", "grok", "opencode"]) {
      expect(providerSupportsPermissionMode(family, "ask_for_approval")).toBe(true);
      expect(providerSupportsPermissionMode(family, "full_access")).toBe(true);
    }
  });

  it("limits automatic review to classifier-backed providers", () => {
    expect(providerSupportsPermissionMode("codex", "approve_for_me")).toBe(true);
    expect(providerSupportsPermissionMode("claude", "approve_for_me")).toBe(true);
    expect(providerSupportsPermissionMode("cursor", "approve_for_me")).toBe(false);
    expect(providerSupportsPermissionMode("opencode", "approve_for_me")).toBe(false);
  });

  it("supports provider-defined Custom permissions for every configured family", () => {
    for (const family of ["codex", "claude", "cursor", "grok", "opencode"] as const) {
      expect(providerSupportsPermissionMode(family, "custom")).toBe(true);
    }
    expect(providerSupportsPermissionMode(null, "custom")).toBe(false);
  });

  it("uses each provider's documented permission filename", () => {
    expect(providerPermissionFileName("codex")).toBe("config.toml");
    expect(providerPermissionFileName("claude")).toBe("settings.json");
    expect(providerPermissionFileName("cursor")).toBe("cli-config.json");
    expect(providerPermissionFileName("grok")).toBe("config.toml");
    expect(providerPermissionFileName("opencode")).toBe("opencode.json/JSONC");
  });
});
