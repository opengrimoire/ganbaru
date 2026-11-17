import { describe, expect, it, vi } from "vitest";
import { PROVIDER_FILE_NOTE_KINDS, type ProviderFileRead, type ProviderPreferenceRead } from "./contracts";
import {
  PROVIDER_DOC_LINKS,
  PROVIDER_FILE_NOTE_LINKS,
  PROVIDER_FILE_NOTE_NAMES,
  PROVIDER_PREFERENCE_NAMES,
  PROVIDER_PREFERENCE_OPTION_NAMES,
  providerFilesFolder,
  providerPreferenceInputValue,
  providerPreferenceName,
  providerPreferenceOptionName,
  providerPreferencesAllowlisted,
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
const preferenceCatalogs = readFileSync(
  fileURLToPath(new NodeUrl("../../../src-tauri/app/src/chat/provider_files/preferences/catalogs.rs", import.meta.url).href),
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
    options: [],
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

  it("labels every preference and option the Codex, Grok, and OpenCode catalogs offer", () => {
    const ids = [...preferenceCatalogs.matchAll(/setting\(\s*&\[([^\]]+)\]/g)].map(([, path]) =>
      [...path.matchAll(/"([^"]+)"/g)].map(([, key]) => key).join("."));
    const options = [...preferenceCatalogs.matchAll(/Choice\(&\[([^\]]+)\]\)/g)].flatMap(([, values]) =>
      [...values.matchAll(/"([^"]+)"/g)].map(([, value]) => value));

    expect(ids).toContain("web_search");
    expect(ids).toContain("toolset.bash.timeout_secs");
    expect(ids).toContain("compaction.keep.tokens");
    expect(ids.filter((id) => providerPreferenceName(id) === null)).toEqual([]);
    expect(options).toContain("save-all");
    expect(options.filter((option) => providerPreferenceOptionName(option) === null)).toEqual([]);
  });

  it("labels choice values by meaning and leaves unknown ones unlabeled", () => {
    expect(providerPreferenceOptionName("save-all")).toBe("saveAll");
    expect(providerPreferenceOptionName("disabled")).toBe("off");
    expect(providerPreferenceOptionName("disable")).toBe("off");
    expect(providerPreferenceOptionName("hasOwnProperty")).toBeNull();
    expect(providerPreferenceOptionName("future")).toBeNull();
    expect(Object.keys(PROVIDER_PREFERENCE_OPTION_NAMES)).toContain("notify");
  });

  it("marks only families whose teammates keep an allowlist of preferences", () => {
    expect(providerPreferencesAllowlisted("claude")).toBe(true);
    expect(providerPreferencesAllowlisted("cursor")).toBe(true);
    expect(providerPreferencesAllowlisted("codex")).toBe(false);
    expect(providerPreferencesAllowlisted("grok")).toBe(false);
    expect(providerPreferencesAllowlisted("opencode")).toBe(false);
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

  it("names the deepest folder that holds every provider file", () => {
    expect(providerFilesFolder([{ path: "/home/user/.claude/CLAUDE.md" }, { path: "/home/user/.claude/settings.json" }])).toBe("/home/user/.claude");
    expect(providerFilesFolder([{ path: "/home/user/.config/opencode/AGENTS.md" }, { path: "/home/user/.claude/CLAUDE.md" }])).toBe("/home/user");
    expect(providerFilesFolder([{ path: "C:\\Users\\user\\.codex\\AGENTS.md" }, { path: "C:\\Users\\user\\.codex\\config.toml" }])).toBe("C:\\Users\\user\\.codex");
    expect(providerFilesFolder([{ path: "/etc/a.json" }, { path: "/home/b.json" }])).toBe("/");
  });

  it("names no folder without files or without a shared folder", () => {
    expect(providerFilesFolder([])).toBeNull();
    expect(providerFilesFolder([{ path: "C:\\a\\x.md" }, { path: "D:\\a\\y.md" }])).toBeNull();
    expect(providerFilesFolder([{ path: "AGENTS.md" }])).toBeNull();
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
