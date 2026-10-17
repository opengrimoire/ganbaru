// @vitest-environment jsdom

import { tick } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { composerModelSelection, readComposerModelSelection } from "$lib/chat/composer/model";
import { getChat } from "$lib/stores/chat.svelte";
import {
  claudeModelSettings,
  composer,
  createChatComposerHarness,
  modelSettings,
  multiProviderSettings,
  pointerEvent,
  resetChatComposerTestStore,
} from "$lib/components/chat/ChatComposer.test-support";

vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  listChatPromptCatalog: vi.fn(async () => []),
}));

describe("ChatComposer model controls", () => {
  const { setup, cleanup } = createChatComposerHarness();

  beforeEach(() => {
    resetChatComposerTestStore();
  });

  afterEach(async () => {
    await cleanup();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("keeps model, effort, and speed inside the compact model control", async () => {
    const chat = getChat();
    chat.settings = modelSettings();
    chat.composer = {
      ...composer(),
      providerInstanceId: "codex-local",
      modelSelection: composerModelSelection("gpt-5.6-sol", false, [
        { key: "reasoning_effort", value: { kind: "choice", value: "medium" } },
        { key: "service_tier", value: { kind: "choice", value: "standard" } },
      ]),
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };
    vi.spyOn(chat, "setComposerModel").mockImplementation((modelSelection) => {
      chat.composer = { ...chat.composer, modelSelection };
    });
    const { target } = setup(false);
    const trigger = target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]");
    expect(trigger?.querySelector(".provider-icon")).toBeNull();
    expect(trigger?.textContent).toContain("5.6 Sol");
    expect(trigger?.textContent).toContain("Medium");
    trigger?.click();
    await tick();
    expect(target.querySelector(".model-popover")).not.toBeNull();
    expect(target.querySelector<HTMLElement>(".model-control")?.style.width).toBe("");
    expect(trigger?.querySelector(".model-chevron")?.classList.contains("open")).toBe(true);
    const effortChoices = [...target.querySelectorAll<HTMLButtonElement>(".effort-options button")];
    expect(effortChoices).toHaveLength(7);
    expect(effortChoices[0]?.getAttribute("aria-label")).toBe("5.6 Terra Light");
    expect(effortChoices[2]?.getAttribute("aria-pressed")).toBe("true");
    expect(effortChoices.every((choice) => !choice.hasAttribute("title"))).toBe(true);
    expect(effortChoices.every((choice) => choice.dataset.appTooltipDisabled === "true")).toBe(true);
    const effortKnob = target.querySelector<HTMLElement>(".effort-knob");
    expect(effortKnob).not.toBeNull();
    expect(effortChoices[0]?.style.left).toContain("0% + 0.875rem");
    expect(effortChoices.at(-1)?.style.left).toContain("100% - 0.875rem");
    effortChoices.forEach((choice, index) => {
      vi.spyOn(choice, "getBoundingClientRect").mockReturnValue(new DOMRect(index * 40, 0, 20, 20));
    });
    const effortLadder = target.querySelector<HTMLElement>(".effort-ladder");
    effortLadder?.dispatchEvent(pointerEvent("pointerdown", 90));
    await tick();
    expect(target.querySelector(".effort-footer.holding")).toBeNull();
    effortLadder?.dispatchEvent(pointerEvent("pointermove", 130));
    await tick();
    expect(target.querySelector(".effort-footer.holding")?.textContent).toContain("Faster");
    expect(target.querySelector(".effort-footer.holding")?.textContent).toContain("Smarter");
    effortLadder?.dispatchEvent(pointerEvent("pointermove", 250));
    await tick();
    expect(trigger?.textContent).toContain("Ultra");
    effortLadder?.dispatchEvent(pointerEvent("pointerup", 250));
    await tick();
    expect(target.querySelector(".effort-footer.holding")).toBeNull();
    expect(effortLadder?.classList.contains("handle-hovered")).toBe(true);
    effortLadder?.dispatchEvent(pointerEvent("pointermove", 10));
    await tick();
    expect(effortLadder?.classList.contains("handle-hovered")).toBe(false);
    effortChoices[2]?.click();
    await tick();
    expect(trigger?.textContent).toContain("Medium");
    const fastButton = target.querySelector<HTMLButtonElement>(".fast-button");
    expect(fastButton?.getAttribute("aria-label")).toBe("Enable Fast mode");
    fastButton?.click();
    await tick();
    expect(target.querySelector(".effort-ladder.fast")).not.toBeNull();
    expect(fastButton?.getAttribute("aria-pressed")).toBe("true");
    effortChoices.at(-1)?.click();
    await tick();
    expect(trigger?.textContent).toContain("Ultra");
    expect(target.querySelector(".effort-name.ultra")).not.toBeNull();
    expect(target.querySelector(".effort-ladder.fast.ultra")).not.toBeNull();
    expect(target.querySelector(".fast-button.active.ultra")).not.toBeNull();
    expect(effortKnob?.style.left).toContain("100% - 0.875rem");
    effortChoices[0]?.click();
    await tick();
    expect(trigger?.textContent).toContain("5.6 Terra");
    expect(trigger?.textContent).toContain("Light");
    expect(target.querySelectorAll<HTMLButtonElement>(".effort-options button")[0]?.getAttribute("aria-pressed")).toBe("true");
    expect(effortKnob?.style.left).toContain("0% + 0.875rem");
    const advanced = target.querySelector<HTMLButtonElement>(".advanced-toggle");
    expect(advanced?.textContent).toContain("Advanced");
    advanced?.click();
    await tick();
    expect(target.querySelector(".advanced-view.active")).not.toBeNull();
    expect(target.querySelector<HTMLElement>(".overview-view")?.inert).toBe(true);
    const advancedRows = [...target.querySelectorAll<HTMLButtonElement>(".advanced-list button")];
    expect(advancedRows.some((button) => button.textContent?.includes("EffortLight"))).toBe(true);
    expect(advancedRows.some((button) => button.textContent?.includes("SpeedStandard"))).toBe(true);
    const speedRow = advancedRows.find((button) => button.textContent?.includes("SpeedStandard"));
    speedRow?.dispatchEvent(new MouseEvent("pointerenter"));
    await tick();
    expect(document.querySelector(".model-flyout")?.textContent).toContain("Standard");
    expect(document.querySelector(".model-flyout")?.textContent).toContain("Fast");
    const modelRow = advancedRows.find((button) => button.textContent?.startsWith("Model"));
    modelRow?.dispatchEvent(new MouseEvent("pointerenter"));
    await tick();
    expect(document.querySelector(".model-flyout.positioned")).not.toBeNull();
    expect(document.querySelector(".provider-rail")).toBeNull();
    expect(document.querySelector('[data-model-company="openai"] .model-company-heading svg')).not.toBeNull();
    expect(document.querySelector('[data-model-company="openai"] .model-company-heading')?.textContent).toContain("OpenAI");
    expect(document.querySelector('[data-model-company="anthropic"] .model-company-heading')?.textContent).toContain("Anthropic");
    const openAiHeading = document.querySelector<HTMLButtonElement>('[data-model-company="openai"] .model-company-heading');
    openAiHeading?.click();
    await tick();
    expect(openAiHeading?.getAttribute("aria-expanded")).toBe("false");
    expect(document.querySelector<HTMLElement>('[data-model-company="openai"] .model-company-content')?.inert).toBe(true);
    openAiHeading?.click();
    await tick();
    expect(openAiHeading?.getAttribute("aria-expanded")).toBe("true");
    expect(document.querySelector(".model-picker-provider")).toBeNull();
    const modelList = document.querySelector<HTMLElement>(".model-list");
    if (!modelList) throw new Error("Model list did not render");
    expect(modelList.querySelector(".model-search-row")).not.toBeNull();
    Object.defineProperties(modelList, {
      clientHeight: { configurable: true, value: 200 },
      scrollHeight: { configurable: true, value: 600 },
      scrollTop: { configurable: true, writable: true, value: 0 },
    });
    modelList.dispatchEvent(new Event("scroll"));
    await tick();
    expect(modelList.getAttribute("data-scroll-fade")).toBe("bottom");
    modelList.scrollTop = 200;
    modelList.dispatchEvent(new Event("scroll"));
    await tick();
    expect(modelList.getAttribute("data-scroll-fade")).toBe("both");
    modelList.scrollTop = 400;
    modelList.dispatchEvent(new Event("scroll"));
    await tick();
    expect(modelList.getAttribute("data-scroll-fade")).toBe("top");
    expect(document.querySelector<HTMLInputElement>(".model-search input")?.placeholder).toBe("Search models...");
    expect(document.querySelector(".model-flyout")?.textContent).not.toContain("gpt-5.6-sol");
    expect(document.querySelector(".model-flyout")?.textContent).not.toContain("GPT-5.4 Deprecated");
    expect(document.querySelector(".favorite-company-section .model-company-heading")?.textContent).toContain("Favorites");
    expect(document.querySelector(".favorite-company-section .model-company-content-inner > p")?.textContent).toBe("No favorites yet.");
    const setupAction = document.querySelector<HTMLElement>('[data-model-company="anthropic"] .company-setup');
    expect(setupAction?.textContent).toBe("Configure Anthropic");
    expect(setupAction?.querySelector("small")).toBeNull();
    expect(document.querySelector(".model-flyout")?.textContent).not.toContain("ProviderCodex");
    expect(target.querySelector('[data-chat-field="interaction"]')).toBeNull();
    trigger?.click();
    await tick();
    expect(target.querySelector(".model-popover")).toBeNull();
    trigger?.click();
    await tick();
    expect(target.querySelector(".advanced-view.active")).not.toBeNull();
    expect(document.querySelector(".model-flyout")).toBeNull();
  });

  it("hides Fast when the provider catalog does not support speed selection", async () => {
    const chat = getChat();
    const settings = modelSettings();
    const provider = settings.providerInstances[0];
    if (!provider?.modelCatalog) throw new Error("Model settings require a discovered catalog");
    provider.modelCatalog.models = provider.modelCatalog.models.map((model) => ({
      ...model,
      options: model.options.filter((definition) => !["service_tier", "fastMode"].includes(definition.key)),
    }));
    chat.settings = settings;
    chat.composer = {
      ...composer(),
      providerInstanceId: provider.configuration.instanceId,
      modelSelection: composerModelSelection("gpt-5.6-sol", false, [
        { key: "reasoning_effort", value: { kind: "choice", value: "medium" } },
      ]),
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };

    const { target } = setup(false);
    target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]")?.click();
    await tick();

    expect(target.querySelector(".advanced-toggle")?.textContent).toContain("Advanced");
    expect(target.querySelector(".fast-button")).toBeNull();
    target.querySelector<HTMLButtonElement>(".advanced-toggle")?.click();
    await tick();
    expect([...target.querySelectorAll<HTMLButtonElement>(".advanced-list button")]
      .some((button) => button.textContent?.startsWith("Speed"))).toBe(false);
  });

  it("keeps an option flyout open after selecting a value", async () => {
    const chat = getChat();
    chat.settings = modelSettings();
    chat.composer = {
      ...composer(),
      providerInstanceId: "codex-local",
      modelSelection: composerModelSelection("gpt-5.6-sol", false, []),
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };
    vi.spyOn(chat, "setComposerModel").mockImplementation((modelSelection) => {
      chat.composer = { ...chat.composer, modelSelection };
    });

    const { target } = setup(false);
    target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]")?.click();
    await tick();
    target.querySelector<HTMLButtonElement>(".advanced-toggle")?.click();
    await tick();
    const speedRow = [...target.querySelectorAll<HTMLButtonElement>(".advanced-list button")]
      .find((button) => button.textContent?.startsWith("Speed"));
    speedRow?.click();
    await tick();
    const fastChoice = [...document.querySelectorAll<HTMLButtonElement>(".option-list > button")]
      .find((button) => button.textContent?.startsWith("Fast"));
    fastChoice?.click();
    await tick();
    expect(document.querySelector(".model-flyout")).not.toBeNull();
    expect(readComposerModelSelection(chat.composer.modelSelection).options).toContainEqual({
      key: "service_tier",
      value: { kind: "choice", value: "fast" },
    });
  });

  it("switches provider and model together from the model picker", async () => {
    const chat = getChat();
    chat.settings = multiProviderSettings();
    chat.composer = {
      ...composer(),
      providerInstanceId: "codex-local",
      modelSelection: composerModelSelection("gpt-5.6-sol", false, []),
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };
    vi.spyOn(chat, "setComposerProvider").mockImplementation((providerInstanceId) => {
      chat.composer = { ...chat.composer, providerInstanceId };
    });
    vi.spyOn(chat, "setComposerModel").mockImplementation((modelSelection) => {
      chat.composer = { ...chat.composer, modelSelection };
    });

    const { target } = setup(false);
    target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]")?.click();
    await tick();
    target.querySelector<HTMLButtonElement>(".advanced-toggle")?.click();
    await tick();
    [...target.querySelectorAll<HTMLButtonElement>(".advanced-list button")]
      .find((button) => button.textContent?.startsWith("Model"))?.click();
    await tick();
    expect(document.querySelector('[data-model-company="openai"]')).not.toBeNull();
    expect(document.querySelector('[data-model-company="anthropic"]')).not.toBeNull();
    const claudeModel = [...document.querySelectorAll<HTMLButtonElement>('[data-model-company="anthropic"] .model-choice')]
      .find((button) => button.textContent?.includes("Opus 4.8"));
    claudeModel?.click();
    await tick();

    expect(chat.composer.providerInstanceId).toBe("claude");
    expect(readComposerModelSelection(chat.composer.modelSelection).modelId).toBe("default");
    expect(document.querySelector(".model-flyout")).not.toBeNull();
    expect(target.querySelector(".model-popover")).not.toBeNull();
  });

  it("shows and updates favorites across providers without exposing raw model IDs", async () => {
    const chat = getChat();
    const settings = multiProviderSettings();
    const codex = settings.providerInstances.find((entry) => entry.configuration.instanceId === "codex-local");
    const claude = settings.providerInstances.find((entry) => entry.configuration.instanceId === "claude");
    if (!codex || !claude) throw new Error("Favorite fixtures require Codex and Claude");
    codex.configuration.favoriteModelIds = ["gpt-5.6-sol"];
    claude.configuration.favoriteModelIds = ["default"];
    chat.settings = settings;
    chat.composer = {
      ...composer(),
      providerInstanceId: "codex-local",
      modelSelection: composerModelSelection("gpt-5.6-sol", false, []),
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };
    const updateModels = vi.spyOn(chat, "updateModels").mockResolvedValue();

    const { target } = setup(false);
    target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]")?.click();
    await tick();
    target.querySelector<HTMLButtonElement>(".advanced-toggle")?.click();
    await tick();
    [...target.querySelectorAll<HTMLButtonElement>(".advanced-list button")]
      .find((button) => button.textContent?.startsWith("Model"))?.click();
    await tick();

    const providerModelRow = [...document.querySelectorAll<HTMLElement>('[data-model-company="openai"] .model-row')]
      .find((row) => row.textContent?.includes("5.6 Sol"));
    expect(providerModelRow?.querySelector(".model-provider-caption")).toBeNull();
    const favoriteText = document.querySelector(".favorite-company-section")?.textContent ?? "";
    expect(favoriteText).toContain("5.6 Sol");
    expect(favoriteText).toContain("Opus 4.8");
    expect(favoriteText).not.toContain("gpt-5.6-sol");
    expect(document.querySelectorAll(".model-provider-caption")).toHaveLength(0);
    expect(document.querySelectorAll(".favorite-source-section")).toHaveLength(0);
    expect(document.querySelectorAll(".favorite-model-label")).toHaveLength(2);

    document.querySelector<HTMLButtonElement>('.favorite-company-section .model-favorite[aria-label="Favorite: 5.6 Sol"]')?.click();
    await tick();
    expect(updateModels).toHaveBeenCalledWith("codex-local", [], []);
  });

  it("selects a provider's recommended model with medium effort for a fresh composer", async () => {
    const chat = getChat();
    chat.settings = claudeModelSettings();
    chat.composer = {
      ...composer(),
      providerInstanceId: "claude",
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };
    vi.spyOn(chat, "setComposerModel").mockImplementation((modelSelection) => {
      chat.composer = { ...chat.composer, modelSelection };
    });

    const { target } = setup(false);
    await tick();
    await tick();

    const selection = readComposerModelSelection(chat.composer.modelSelection);
    expect(selection.modelId).toBe("default");
    expect(selection.providerManaged).toBe(false);
    expect(selection.options).toEqual([{
      key: "effort",
      value: { kind: "choice", value: "medium" },
    }, {
      key: "fastMode",
      value: { kind: "boolean", value: false },
    }]);
    const trigger = target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]");
    expect(trigger?.textContent).toContain("Opus 4.8");
    expect(trigger?.textContent).toContain("Medium");

    trigger?.click();
    await tick();
    expect(target.querySelector(".fast-button")).not.toBeNull();
    const choices = [...target.querySelectorAll<HTMLButtonElement>(".effort-options button")];
    expect(choices).toHaveLength(5);
    expect(choices[1]?.getAttribute("aria-pressed")).toBe("true");
    expect(choices[0]?.style.left).toContain("0% + 0.875rem");
    expect(choices.at(-1)?.style.left).toContain("100% - 0.875rem");
    const knob = target.querySelector<HTMLElement>(".effort-knob");
    choices[0]?.click();
    await tick();
    expect(knob?.style.left).toContain("0% + 0.875rem");
    choices.at(-1)?.click();
    await tick();
    expect(knob?.style.left).toContain("100% - 0.875rem");

    const fastButton = target.querySelector<HTMLButtonElement>(".fast-button");
    expect(fastButton?.getAttribute("aria-pressed")).toBe("false");
    fastButton?.click();
    await tick();
    expect(readComposerModelSelection(chat.composer.modelSelection).options).toContainEqual({
      key: "fastMode",
      value: { kind: "boolean", value: true },
    });
    expect(fastButton?.getAttribute("aria-pressed")).toBe("true");
  });

  it("keeps a neutral five-stop track for models without effort levels", async () => {
    const chat = getChat();
    const settings = claudeModelSettings();
    const provider = settings.providerInstances[0];
    if (!provider?.modelCatalog) throw new Error("Claude model settings require a discovered catalog");
    provider.modelCatalog.models = [{
      id: "haiku",
      displayName: "Haiku 4.5",
      description: "Fastest for quick answers",
      contextLimit: null,
      availability: "available",
      capabilities: [],
      custom: false,
      options: [],
    }];
    chat.settings = settings;
    chat.composer = {
      ...composer(),
      providerInstanceId: "claude",
      modelSelection: composerModelSelection("haiku", false, []),
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };

    const { target } = setup(false);
    target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]")?.click();
    await tick();

    const dummyLadder = target.querySelector<HTMLElement>(".effort-ladder.dummy");
    expect(dummyLadder?.getAttribute("aria-disabled")).toBe("true");
    expect(dummyLadder?.getAttribute("aria-label")).toBe("This model does not provide effort levels");
    expect(dummyLadder?.querySelectorAll(".effort-stop")).toHaveLength(5);
    expect(dummyLadder?.querySelector(".effort-fill")).toBeNull();
    expect(dummyLadder?.querySelector(".effort-knob")).toBeNull();
  });

  it("selects the strongest model from the first healthy provider and defaults permissions", async () => {
    const chat = getChat();
    const settings = modelSettings();
    const firstProvider = settings.providerInstances[0];
    if (!firstProvider?.modelCatalog) throw new Error("Model settings require a discovered catalog");
    firstProvider.modelCatalog.models.reverse();
    chat.settings = settings;
    vi.spyOn(chat, "setComposerProvider").mockImplementation((providerInstanceId) => {
      chat.composer = { ...chat.composer, providerInstanceId };
    });
    vi.spyOn(chat, "setComposerModel").mockImplementation((modelSelection) => {
      chat.composer = { ...chat.composer, modelSelection };
    });
    vi.spyOn(chat, "setComposerModes").mockImplementation((safetyMode, interactionMode) => {
      chat.composer = { ...chat.composer, safetyMode, interactionMode };
    });

    const { target } = setup(false);
    await tick();
    await tick();
    await tick();

    expect(chat.composer.providerInstanceId).toBe("codex-local");
    expect(readComposerModelSelection(chat.composer.modelSelection).modelId).toBe("gpt-5.6-sol");
    expect(chat.composer.safetyMode).toBe("ask_for_approval");
    expect(chat.composer.interactionMode).toBe("build");
    expect(target.querySelector('[data-chat-field="safety"]')?.textContent).toContain("Ask for approval");
    expect(target.querySelector("[data-chat-model-trigger]")?.textContent).toContain("5.6 Sol");
  });

  it("previews disabled model controls when no provider is configured", async () => {
    const chat = getChat();
    const settings = modelSettings();
    settings.providerInstances = [];
    chat.settings = settings;
    vi.spyOn(chat, "setComposerModes").mockImplementation((safetyMode, interactionMode) => {
      chat.composer = { ...chat.composer, safetyMode, interactionMode };
    });

    const { target } = setup(false);
    await tick();
    await tick();

    expect(chat.composer.providerInstanceId).toBeNull();
    expect(chat.composer.safetyMode).toBe("ask_for_approval");
    const trigger = target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]");
    expect(trigger?.textContent).toContain("Choose provider");
    trigger?.click();
    await tick();
    await tick();

    expect(target.querySelector(".advanced-view.active")).not.toBeNull();
    expect(target.querySelector(".advanced-list")?.textContent).toContain("Model");
    const rows = [...target.querySelectorAll<HTMLButtonElement>(".advanced-list button")];
    expect(rows[0]?.disabled).toBe(false);
    expect(rows[1]?.disabled).toBe(true);
    expect(rows[2]?.disabled).toBe(true);
    target.querySelector<HTMLButtonElement>(".advanced-heading")?.click();
    await tick();
    const dummyLadder = target.querySelector<HTMLElement>(".effort-ladder.dummy");
    expect(dummyLadder?.getAttribute("aria-disabled")).toBe("true");
    expect(dummyLadder?.querySelectorAll(".effort-stop")).toHaveLength(5);
    expect(dummyLadder?.querySelector(".effort-knob")).toBeNull();
    expect(target.querySelector<HTMLButtonElement>(".fast-button.dummy")?.disabled).toBe(true);
  });
});
