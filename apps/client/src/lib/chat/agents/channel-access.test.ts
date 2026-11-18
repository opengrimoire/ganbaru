import { describe, expect, it } from "vitest";
import type { ChatAgentResolvedAccessRead, ChatChannelModelOverrideRead } from "$lib/chat/contracts";
import { en } from "$lib/i18n/messages/en";
import { es } from "$lib/i18n/messages/es";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import {
  channelModelOverride,
  chatAccessOptions,
  chatAccessSourceLabel,
  chatAgentBehaviorLine,
  chatAgentDefaultModelName,
  chatScopeSourceLine,
  effectiveAccessEditsFiles,
  ownAccessAvailable,
} from "./channel-access";

const tEn: Translate = ((key, ...args) => translateFromPartialCatalog(en, key, ...args)) as Translate;
const tEs: Translate = ((key, ...args) => translateFromPartialCatalog(es, key, ...args)) as Translate;

function resolved(overrides: Partial<ChatAgentResolvedAccessRead> = {}): ChatAgentResolvedAccessRead {
  return {
    agentId: "agent-1",
    requested: "ask",
    effective: "ask",
    downgradeReason: null,
    installed: true,
    ownOffered: true,
    ...overrides,
  };
}

describe("effectiveAccessEditsFiles", () => {
  it("allows edits only at levels that edit files", () => {
    expect(effectiveAccessEditsFiles("talk")).toBe(false);
    expect(effectiveAccessEditsFiles("read")).toBe(false);
    expect(effectiveAccessEditsFiles(null)).toBe(false);
    expect(effectiveAccessEditsFiles("edit_only")).toBe(true);
    expect(effectiveAccessEditsFiles("ask")).toBe(true);
    expect(effectiveAccessEditsFiles("own")).toBe(true);
  });
});

describe("ownAccessAvailable", () => {
  it("stays available at scopes without agent information and in channels without installed agents", () => {
    expect(ownAccessAvailable(null)).toBe(true);
    expect(ownAccessAvailable([])).toBe(true);
    expect(ownAccessAvailable([resolved({ installed: false, ownOffered: false })])).toBe(true);
  });

  it("needs one installed agent that can confine its commands", () => {
    expect(ownAccessAvailable([resolved({ ownOffered: false })])).toBe(false);
    expect(ownAccessAvailable([resolved({ ownOffered: false }), resolved({ agentId: "agent-2" })])).toBe(true);
    expect(ownAccessAvailable([
      resolved({ ownOffered: false }),
      resolved({ agentId: "agent-2", installed: false, ownOffered: false }),
    ])).toBe(false);
  });
});

describe("chatAccessOptions", () => {
  it("lists the four levels narrowest first with one-sentence descriptions in both languages", () => {
    for (const t of [tEn, tEs]) {
      const options = chatAccessOptions(t, { ownAvailable: true, markDefault: false });
      expect(options.map((option) => option.value)).toEqual(["talk", "read", "ask", "own"]);
      for (const option of options) {
        expect(option.label).not.toMatch(/^chat\./u);
        expect(option.description.trim().split(/[.!?](\s|$)/u).filter((part) => part.trim()).length).toBe(1);
        expect(option.description).not.toMatch(/focus|enfoque/iu);
        expect(option.disabled).toBe(false);
      }
    }
  });

  it("disables only Work on their own when this device cannot confine commands", () => {
    const options = chatAccessOptions(tEn, { ownAvailable: false, markDefault: false });
    expect(options.filter((option) => option.disabled).map((option) => option.value)).toEqual(["own"]);
  });

  it("marks the default level only when asked", () => {
    expect(chatAccessOptions(tEn, { ownAvailable: true, markDefault: true }).map((option) => option.label)).toEqual([
      "Talk only",
      "Read only",
      "Ask before commands (default)",
      "Work on their own",
    ]);
    expect(chatAccessOptions(tEn, { ownAvailable: true, markDefault: false })[2]?.label).toBe("Ask before commands");
  });
});

describe("chatAccessSourceLabel", () => {
  it("names each scope a resolved access comes from", () => {
    expect(chatAccessSourceLabel(tEn, "project")).toBe("From project");
    expect(chatAccessSourceLabel(tEn, "app")).toBe("From app settings");
    expect(chatAccessSourceLabel(tEn, "channel")).toBe("Set for this channel");
    expect(chatAccessSourceLabel(tEs, "app")).toBe("De la configuración de la app");
  });
});

describe("chatScopeSourceLine", () => {
  it("names the inherited source while a scope follows its parent", () => {
    expect(chatScopeSourceLine(tEn, "channel", null, "project")).toBe("From project");
    expect(chatScopeSourceLine(tEn, "channel", null, "app")).toBe("From app settings");
    expect(chatScopeSourceLine(tEn, "project", null, "app")).toBe("From app settings");
  });

  it("says the scope sets its own access once it chooses one", () => {
    expect(chatScopeSourceLine(tEn, "channel", "read", "project")).toBe("Set for this channel");
    expect(chatScopeSourceLine(tEn, "project", "own", "app")).toBe("Set for this project");
    expect(chatScopeSourceLine(tEs, "project", "talk", "app")).toBe("Definido para este proyecto");
  });
});

describe("chatAgentBehaviorLine", () => {
  it("describes the level an agent works at when nothing narrows it", () => {
    expect(chatAgentBehaviorLine(tEn, resolved(), "Claude")).toBe("Asks before every command");
    expect(chatAgentBehaviorLine(tEn, resolved({ requested: "talk", effective: "talk" }), "Claude")).toBe("Replies from the channel alone");
    expect(chatAgentBehaviorLine(tEn, resolved({ requested: "own", effective: "own" }), "Claude"))
      .toBe("Runs commands without asking, confined to the project's folders");
  });

  it("names the provider and the reason for every downgrade", () => {
    expect(chatAgentBehaviorLine(tEn, resolved({ effective: "read", downgradeReason: "cannot_ask_before_commands" }), "Cursor"))
      .toBe("Read only here: Cursor cannot ask before commands");
    expect(chatAgentBehaviorLine(tEn, resolved({ effective: "edit_only", downgradeReason: "cannot_ask_before_commands" }), "Grok"))
      .toBe("Edits files without running commands here: Grok cannot ask before commands");
    expect(chatAgentBehaviorLine(tEn, resolved({ requested: "own", effective: "ask", downgradeReason: "cannot_confine_commands" }), "Codex"))
      .toBe("Asks before every command here: Codex cannot confine commands");
    expect(chatAgentBehaviorLine(tEs, resolved({ effective: "read", downgradeReason: "cannot_ask_before_commands" }), "Cursor"))
      .toBe("Solo lectura aquí: Cursor no puede preguntar antes de los comandos");
  });

  it("says when an agent cannot work here at all", () => {
    expect(chatAgentBehaviorLine(tEn, resolved({ requested: "talk", effective: null, downgradeReason: "cannot_isolate" }), "Cursor"))
      .toBe("Cannot work here on this device: Cursor cannot work without the project's folders");
    expect(chatAgentBehaviorLine(tEn, resolved({ effective: null }), "Cursor")).toBe("Cannot work here on this device");
  });

  it("puts a missing installation before any level", () => {
    expect(chatAgentBehaviorLine(tEn, resolved({ installed: false, effective: null }), "Codex"))
      .toBe("Not installed on this device: work waits for Start here");
  });
});

describe("chatAgentDefaultModelName", () => {
  const catalog = { models: [{ id: "gpt-5", displayName: "GPT-5" }], recommendedModelId: null };

  it("uses the catalog name of the agent's model, or the recommended one when the catalog lacks it", () => {
    expect(chatAgentDefaultModelName(tEn, { providerManagedModel: false, modelId: "gpt-5" }, catalog)).toBe("GPT-5");
    expect(chatAgentDefaultModelName(tEn, { providerManagedModel: false, modelId: "o9" }, catalog)).toBe("the model set in the provider");
    expect(chatAgentDefaultModelName(tEn, { providerManagedModel: false, modelId: "o9" }, { ...catalog, recommendedModelId: "gpt-5" })).toBe("GPT-5");
    expect(chatAgentDefaultModelName(tEn, { providerManagedModel: false, modelId: "gpt-5" }, null)).toBe("gpt-5");
  });

  it("names the provider's configured model when the agent has no model and no catalog", () => {
    expect(chatAgentDefaultModelName(tEn, null, null)).toBe("the model set in the provider");
    expect(chatAgentDefaultModelName(tEn, { providerManagedModel: true, modelId: null }, null)).toBe("the model set in the provider");
  });

  it("names the recommended model when the agent has no model of its own", () => {
    const claude = { models: [{ id: "fable", displayName: "Fable 9" }, { id: "opus", displayName: "Opus 9" }], recommendedModelId: "opus" };
    expect(chatAgentDefaultModelName(tEn, null, claude)).toBe("Opus 9");
    expect(chatAgentDefaultModelName(tEn, { providerManagedModel: true, modelId: null }, claude)).toBe("Opus 9");
    expect(chatAgentDefaultModelName(tEn, { providerManagedModel: false, modelId: "fable" }, claude)).toBe("Fable 9");
  });
});

describe("channelModelOverride", () => {
  const overrides: ChatChannelModelOverrideRead[] = [
    { agentId: "agent-1", model: { providerManagedModel: false, modelId: "gpt-5", modelOptions: [] } },
    { agentId: "agent-2", model: { providerManagedModel: true, modelId: null, modelOptions: [] } },
  ];

  it("returns the overriding model for an agent and null otherwise", () => {
    expect(channelModelOverride(overrides, "agent-1")).toBe("gpt-5");
    expect(channelModelOverride(overrides, "agent-2")).toBeNull();
    expect(channelModelOverride(overrides, "agent-3")).toBeNull();
  });
});
