// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { MusicSoundscapeDefinition, MusicSoundscapePreset, MusicSoundscapePresetWrite } from "$lib/music/soundscape/contracts";
import MusicSoundscapePresetEditor from "./MusicSoundscapePresetEditor.svelte";

const savePreset = vi.fn(async (write: MusicSoundscapePresetWrite): Promise<MusicSoundscapePreset> => ({
  id: write.id, name: write.name, icon: write.icon, soundscapeIds: write.soundscapeIds,
  createdAtMs: 1, updatedAtMs: write.updatedAtMs, version: (write.expectedVersion ?? 0) + 1,
}));
const toggleSelection = vi.fn();
const playPreset = vi.fn(async () => undefined);

function sound(id: string, name: string, groupId: string | null = null): MusicSoundscapeDefinition {
  return {
    id, sourceKind: "local-loop", generatedKind: null, bundledIdentity: null, name, icon: "lucide:audio-lines",
    groupId, availability: "available", localPath: `/sounds/${id}.ogg`, createdAtMs: 1, updatedAtMs: 1, version: 1,
  };
}

const definitions = [sound("rain", "Porch rain", "outside"), sound("cafe", "Café"), sound("fire", "Fireplace")];

vi.mock("$lib/stores/soundscape.svelte", () => ({
  getSoundscapeStore: () => ({
    saving: false,
    savePreset,
    toggleSelection,
    playPreset,
    togglePlayback: vi.fn(),
    definitions,
    groups: [{ id: "outside", name: "Outside", icon: "lucide:trees", createdAtMs: 1, updatedAtMs: 1, version: 1 }],
    persisted: { activeIds: ["fire"] },
    snapshot: { status: "playing" },
  }),
}));

const evening: MusicSoundscapePreset = {
  id: "evening", name: "Evening", icon: "lucide:moon", soundscapeIds: ["rain", "cafe"],
  createdAtMs: 1, updatedAtMs: 1, version: 3,
};

let component: ReturnType<typeof mount> | undefined;
let target: HTMLDivElement;

beforeAll(() => {
  Element.prototype.scrollIntoView = () => undefined;
});

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  target?.remove();
  savePreset.mockClear();
  toggleSelection.mockClear();
  playPreset.mockClear();
});

async function render(preset: MusicSoundscapePreset | null = null) {
  target = document.createElement("div");
  document.body.append(target);
  const onDone = vi.fn<(preset: MusicSoundscapePreset | null) => void>();
  component = mount(MusicSoundscapePresetEditor, { target, props: { preset, onDone } });
  await tick();
  return onDone;
}

function nameField(): HTMLInputElement {
  return target.querySelector<HTMLInputElement>('input[aria-label="Preset name"]')!;
}

function soundTile(name: string): HTMLButtonElement {
  return [...target.querySelectorAll<HTMLButtonElement>("button[aria-pressed]")].find((button) => button.textContent?.includes(name))!;
}

function saveButton(): HTMLButtonElement {
  return target.querySelector<HTMLButtonElement>('button[type="submit"]')!;
}

describe("Background sound preset editor", () => {
  it("starts a new preset empty instead of copying what is playing", async () => {
    await render();
    expect([...target.querySelectorAll('button[aria-pressed="true"]')]).toHaveLength(0);
    expect(target.textContent).toContain("0 of 16");
  });

  it("picks sounds into the draft without changing playback and saves them in pick order", async () => {
    const onDone = await render();
    nameField().value = "Deep focus";
    nameField().dispatchEvent(new Event("input", { bubbles: true }));
    soundTile("Café").click();
    soundTile("Porch rain").click();
    await tick();
    expect(soundTile("Café").getAttribute("aria-pressed")).toBe("true");
    expect(toggleSelection).not.toHaveBeenCalled();
    expect(playPreset).not.toHaveBeenCalled();
    saveButton().click();
    await vi.waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(savePreset).toHaveBeenCalledWith(expect.objectContaining({ name: "Deep focus", soundscapeIds: ["cafe", "rain"], expectedVersion: null }));
  });

  it("edits a preset from its own sounds, not the current selection", async () => {
    await render(evening);
    expect(nameField().value).toBe("Evening");
    expect(soundTile("Porch rain").getAttribute("aria-pressed")).toBe("true");
    expect(soundTile("Fireplace").getAttribute("aria-pressed")).toBe("false");
    soundTile("Porch rain").click();
    await tick();
    saveButton().click();
    await vi.waitFor(() => expect(savePreset).toHaveBeenCalledTimes(1));
    expect(savePreset.mock.calls[0][0]).toMatchObject({ id: "evening", soundscapeIds: ["cafe"], expectedVersion: 3 });
  });

  it("groups sounds the way the builder does", async () => {
    await render();
    const headings = [...target.querySelectorAll("h4")].map((heading) => heading.textContent);
    expect(headings).toEqual(["Outside", "Other sounds"]);
  });

  it("plays the draft only when Listen is pressed", async () => {
    await render(evening);
    [...target.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent?.includes("Listen"))!.click();
    expect(playPreset).toHaveBeenCalledWith({ soundscapeIds: ["rain", "cafe"] });
  });

  it("cannot save without a name or without sounds", async () => {
    await render();
    soundTile("Café").click();
    await tick();
    expect(saveButton().disabled).toBe(true);
  });

  it("cancels with Escape", async () => {
    const onDone = await render(evening);
    nameField().dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    expect(onDone).toHaveBeenCalledWith(null);
    expect(savePreset).not.toHaveBeenCalled();
  });
});
