import { describe, expect, it } from "vitest";
import type { ChatPromptCatalogEntry } from "$lib/chat/contracts";
import {
  channelModelChoices,
  genericModelCommandEntry,
  isModelCommandEntry,
  mergePromptCatalog,
  modelCommandEntries,
} from "./command-model";

const models = [
  { id: "gpt-5", displayName: "GPT-5" },
  { id: "gpt-5-mini", displayName: "GPT-5 mini" },
];
const describeModel = (name: string) => `Use ${name} for this work`;

function appCommand(value: string): ChatPromptCatalogEntry {
  return { value, label: value.slice(1), description: null, argumentHint: null, kind: "command", source: "app", stale: false };
}

describe("modelCommandEntries", () => {
  it("lists one /model suggestion per model the agent's provider offers", () => {
    expect(modelCommandEntries(models, describeModel)).toEqual([
      { value: "/model gpt-5", label: "/model gpt-5", description: "Use GPT-5 for this work", argumentHint: null, kind: "command", source: "app", stale: false },
      { value: "/model gpt-5-mini", label: "/model gpt-5-mini", description: "Use GPT-5 mini for this work", argumentHint: null, kind: "command", source: "app", stale: false },
    ]);
  });

  it("offers nothing when the provider's catalog is unknown here", () => {
    expect(modelCommandEntries([], describeModel)).toEqual([]);
  });

  it("shows the model suggestions as the user types /mo and narrows them by model", () => {
    const entries = [appCommand("/status"), ...modelCommandEntries(models, describeModel)];
    expect(mergePromptCatalog(entries, [], "command", "mo").map((entry) => entry.value))
      .toEqual(["/model gpt-5", "/model gpt-5-mini"]);
    expect(mergePromptCatalog(entries, [], "command", "mini").map((entry) => entry.value))
      .toEqual(["/model gpt-5-mini"]);
  });
});

describe("isModelCommandEntry", () => {
  it("tells model choices, which go into the draft, from app commands that run", () => {
    const [choice] = modelCommandEntries(models, describeModel);
    expect(isModelCommandEntry(choice)).toBe(true);
    expect(isModelCommandEntry(appCommand("/status"))).toBe(false);
    expect(isModelCommandEntry(appCommand("/model"))).toBe(true);
    expect(isModelCommandEntry({ ...choice, source: "provider" })).toBe(false);
    expect(isModelCommandEntry(genericModelCommandEntry("/model", "Choose the model", "model"))).toBe(true);
  });
});

describe("genericModelCommandEntry", () => {
  it("keeps /model discoverable with its argument hint when no model list is known", () => {
    expect(genericModelCommandEntry("/model", "Choose the model for this work", "model")).toEqual({
      value: "/model",
      label: "/model",
      description: "Choose the model for this work",
      argumentHint: "model",
      kind: "command",
      source: "app",
      stale: false,
    });
  });
});

describe("channelModelChoices", () => {
  it("joins the channel agents' catalogs, first occurrence first, without repeats", () => {
    const opus = { id: "opus", displayName: "Opus" };
    expect(channelModelChoices([models, [opus, models[0]]]).map((model) => model.id))
      .toEqual(["gpt-5", "gpt-5-mini", "opus"]);
  });

  it("returns nothing without catalogs", () => {
    expect(channelModelChoices([])).toEqual([]);
    expect(channelModelChoices([[], []])).toEqual([]);
  });
});
