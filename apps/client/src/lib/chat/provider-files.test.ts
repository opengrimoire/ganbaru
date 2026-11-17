import { describe, expect, it, vi } from "vitest";
import { PROVIDER_FILE_NOTE_KINDS, type ProviderFileRead, type ProviderPreferenceRead } from "./contracts";
import {
  PROVIDER_DOC_LINKS,
  PROVIDER_FILE_NOTE_LINKS,
  PROVIDER_FILE_NOTE_NAMES,
  PROVIDER_PREFERENCE_NAMES,
  providerFilesNote,
  providerPreferenceInputValue,
  providerPreferenceName,
  splitProviderFiles,
} from "./provider-files";

// Load test-only Node APIs without introducing Node globals into the browser type environment.
const { readFileSync } = await vi.importActual<{
  readFileSync: (path: string, encoding: "utf8") => string;
}>("node:fs");
const { fileURLToPath, URL: NodeUrl } = await vi.importActual<{
  URL: typeof URL;
  fileURLToPath: (url: string) => string;
}>("node:url");
const desktopCapability = readFileSync(
  fileURLToPath(new NodeUrl("../../../src-tauri/capabilities/desktop.json", import.meta.url).href),
  "utf8",
);

function preference(overrides: Partial<ProviderPreferenceRead>): ProviderPreferenceRead {
  return {
    id: "language",
    valueType: "text",
    variable: false,
    value: null,
    ignored: false,
    maxBytes: 64,
    multiline: false,
    emptyAllowed: false,
    ...overrides,
  };
}

function file(fileId: string, kind: ProviderFileRead["kind"]): ProviderFileRead {
  return {
    fileId,
    name: fileId,
    kind,
    format: kind === "instructions" ? "markdown" : "json",
    path: `/home/user/${fileId}`,
    exists: true,
    contents: "",
    revision: "r",
    preferences: null,
  };
}

describe("provider files", () => {
  it("clears blank input unless an empty text has a meaning", () => {
    expect(providerPreferenceInputValue(preference({}), "")).toBeNull();
    expect(providerPreferenceInputValue(preference({ emptyAllowed: true }), "")).toBe("");
    expect(providerPreferenceInputValue(preference({ emptyAllowed: true }), "  Co-authored  ")).toBe("  Co-authored  ");
    expect(providerPreferenceInputValue(preference({ valueType: "count" }), "  8000 ")).toBe("8000");
    expect(providerPreferenceInputValue(preference({ valueType: "percentage" }), "   ")).toBeNull();
  });

  it("names only preferences this build describes", () => {
    expect(providerPreferenceName("CLAUDE_CODE_DISABLE_1M_CONTEXT")).toBe("disableLongContext");
    expect(providerPreferenceName("network.useHttp1ForAgent")).toBe("http1");
    expect(providerPreferenceName("toString")).toBeNull();
    expect(providerPreferenceName("FUTURE_SETTING")).toBeNull();
    const names = Object.values(PROVIDER_PREFERENCE_NAMES);
    expect(new Set(names).size).toBe(names.length);
  });

  it("explains how each family's teammates use their files", () => {
    expect(providerFilesNote("claude")).toBe("allowlist");
    expect(providerFilesNote("cursor")).toBe("native");
    expect(providerFilesNote("codex")).toBe("native");
    expect(providerFilesNote("grok")).toBe("native");
    expect(providerFilesNote("opencode")).toBe("native");
    expect(providerFilesNote("acp")).toBeNull();
  });

  it("groups instruction and configuration files in backend order", () => {
    const groups = splitProviderFiles([
      file("configuration", "configuration"),
      file("instructions", "instructions"),
      file("global_configuration", "configuration"),
    ]);
    expect(groups.instructions.map((entry) => entry.fileId)).toEqual(["instructions"]);
    expect(groups.configuration.map((entry) => entry.fileId)).toEqual(["configuration", "global_configuration"]);
  });
});

describe("provider file notes", () => {
  it("names and links every note kind the backend reports", () => {
    for (const kind of PROVIDER_FILE_NOTE_KINDS) {
      expect(PROVIDER_FILE_NOTE_NAMES[kind]).toBeTruthy();
      expect(PROVIDER_FILE_NOTE_LINKS[kind]).toMatch(/^https:\/\//);
    }
    const names = Object.values(PROVIDER_FILE_NOTE_NAMES);
    expect(new Set(names).size).toBe(names.length);
  });

  it("keeps every documentation link inside the desktop opener scope", () => {
    const urls = [
      ...Object.values(PROVIDER_DOC_LINKS).flatMap((links) => links.map((link) => link.url)),
      ...Object.values(PROVIDER_FILE_NOTE_LINKS),
    ];
    for (const url of urls) {
      expect(desktopCapability).toContain(`"url": "${url}"`);
    }
  });
});
