import { describe, expect, it } from "vitest";
import {
  createProviderSetupDraft,
  providerConfigurationFromDraft,
  providerInstanceIdFromLabel,
  validateProviderSetup,
} from "./provider-setup";

describe("provider setup", () => {
  it("creates stable editable instance IDs from labels", () => {
    expect(providerInstanceIdFromLabel(" Chloé's Codex ")).toBe("chloe-s-codex");
  });

  it("reports duplicate IDs, environment conflicts, and missing secret references by field", () => {
    const draft = createProviderSetupDraft();
    Object.assign(draft, {
      familyId: "codex",
      label: "Work",
      instanceId: "work",
      executable: "codex",
      providerHome: "/tmp/codex",
      environment: [
        { key: "one", name: "CODEX_HOME", valueType: "text", value: "/other", credentialReference: "" },
        { key: "two", name: "TOKEN", valueType: "secret", value: "", credentialReference: "" },
      ],
    });
    const result = validateProviderSetup(draft, new Set(["work"]));
    expect(result.valid).toBe(false);
    expect(result.fields.instanceId).toContain("already");
    expect(result.fields["environment.one.name"]).toContain("dedicated home field");
    expect(result.fields["environment.two.value"]).toContain("Store");
  });

  it("keeps provider home variables aligned with the dedicated home field", () => {
    for (const [familyId, name] of [
      ["codex", "CODEX_HOME"],
      ["claude", "CLAUDE_CONFIG_DIR"],
    ] as const) {
      const draft = createProviderSetupDraft();
      Object.assign(draft, {
        familyId,
        label: familyId,
        instanceId: familyId,
        executable: familyId,
        environment: [
          { key: "home", name, valueType: "inherit", value: "", credentialReference: "" },
        ],
      });
      expect(validateProviderSetup(draft, new Set()).fields["environment.home.name"]).toContain(
        "dedicated home field",
      );
    }
  });

  it("keeps secret values out of provider configuration", () => {
    const draft = createProviderSetupDraft();
    Object.assign(draft, {
      familyId: "codex",
      label: "Codex",
      instanceId: "codex",
      executable: "codex",
      environment: [
        { key: "one", name: "TOKEN", valueType: "secret", value: "not-serialized", credentialReference: "provider:codex:TOKEN" },
      ],
    });
    const configuration = providerConfigurationFromDraft(draft);
    expect(JSON.stringify(configuration)).not.toContain("not-serialized");
    expect(configuration.credentialReferences.TOKEN).toBe("provider:codex:TOKEN");
  });

  it("preserves enabled and model preferences while editing a connection", () => {
    const draft = createProviderSetupDraft();
    Object.assign(draft, {
      familyId: "codex",
      label: "Work Codex",
      instanceId: "codex-work",
      executable: "/usr/bin/codex",
    });
    const current = {
      ...providerConfigurationFromDraft(draft),
      enabled: false,
      visibleModelIds: ["gpt-5.6-sol"],
      favoriteModelIds: ["gpt-5.6-sol"],
    };

    draft.executable = "/opt/codex/bin/codex";
    const updated = providerConfigurationFromDraft(draft, current);

    expect(updated.executable).toBe("/opt/codex/bin/codex");
    expect(updated.enabled).toBe(false);
    expect(updated.visibleModelIds).toEqual(["gpt-5.6-sol"]);
    expect(updated.favoriteModelIds).toEqual(["gpt-5.6-sol"]);
  });

  it("requires workspace consent for external OpenCode servers", () => {
    const draft = createProviderSetupDraft();
    Object.assign(draft, {
      familyId: "opencode",
      label: "OpenCode",
      instanceId: "opencode",
      executable: "opencode",
      providerConfig: { schemaVersion: 1, value: { mode: "external", serverUrl: "https://example.com" } },
    });
    expect(validateProviderSetup(draft, new Set()).fields["providerConfig.confirmExternalWorkspaceAccess"]).toBeDefined();
    draft.providerConfig = { schemaVersion: 1, value: { mode: "external", serverUrl: "https://example.com", confirmExternalWorkspaceAccess: true } };
    expect(validateProviderSetup(draft, new Set()).valid).toBe(true);
  });

  it("requires an explicit override for non-loopback OpenCode HTTP", () => {
    const draft = createProviderSetupDraft();
    Object.assign(draft, {
      familyId: "opencode",
      label: "OpenCode",
      instanceId: "opencode",
      executable: "opencode",
      providerConfig: { schemaVersion: 1, value: { mode: "external", serverUrl: "http://example.com", confirmExternalWorkspaceAccess: true } },
    });
    expect(validateProviderSetup(draft, new Set()).fields["providerConfig.allowInsecureExternalHttp"]).toBeDefined();
    draft.providerConfig = {
      schemaVersion: 1,
      value: {
        mode: "external",
        serverUrl: "http://example.com",
        confirmExternalWorkspaceAccess: true,
        allowInsecureExternalHttp: true,
      },
    };
    expect(validateProviderSetup(draft, new Set()).valid).toBe(true);
  });

  it("accepts a loopback HTTP OpenCode server URL", () => {
    const draft = createProviderSetupDraft();
    Object.assign(draft, {
      familyId: "opencode",
      label: "OpenCode",
      instanceId: "opencode",
      executable: "opencode",
      providerConfig: { schemaVersion: 1, value: { mode: "external", serverUrl: "http://127.0.0.1:4096", confirmExternalWorkspaceAccess: true } },
    });
    expect(validateProviderSetup(draft, new Set()).valid).toBe(true);
  });

  it("rejects the predecessor OpenCode endpoint field", () => {
    const draft = createProviderSetupDraft();
    Object.assign(draft, {
      familyId: "opencode",
      label: "OpenCode",
      instanceId: "opencode",
      executable: "opencode",
      providerConfig: { schemaVersion: 1, value: { mode: "external", endpoint: "http://127.0.0.1:4096", confirmExternalWorkspaceAccess: true } },
    });
    expect(validateProviderSetup(draft, new Set()).fields["providerConfig.serverUrl"]).toBeDefined();
  });

  it("requires an explicit OpenCode connection mode", () => {
    const draft = createProviderSetupDraft();
    Object.assign(draft, {
      familyId: "opencode",
      label: "OpenCode",
      instanceId: "opencode",
      executable: "opencode",
      providerConfig: { schemaVersion: 1, value: { serverUrl: "https://example.com" } },
    });
    expect(validateProviderSetup(draft, new Set()).fields["providerConfig.mode"]).toBeDefined();
  });
});
