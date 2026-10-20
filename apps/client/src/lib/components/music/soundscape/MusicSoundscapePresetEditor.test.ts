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
const beginPreview = vi.fn(async () => undefined);
const previewSounds = vi.fn(async (_ids: readonly string[]) => undefined);
const endPreview = vi.fn(async () => undefined);
const player = {
  isPlaying: false,
  pausePlayback: vi.fn(async () => { player.isPlaying = false; }),
  playPlayback: vi.fn(async () => { player.isPlaying = true; }),
};

function sound(id: string, name: string, groupId: string | null = null): MusicSoundscapeDefinition {
  return {
    id, sourceKind: "local-loop", generatedKind: null, bundledIdentity: null, name, icon: "lucide:audio-lines",
    groupId, availability: "available", localPath: `/sounds/${id}.ogg`, createdAtMs: 1, updatedAtMs: 1, version: 1,
  };
}

const baseDefinitions = [sound("rain", "Porch rain", "outside"), sound("cafe", "Café"), sound("fire", "Fireplace")];
let definitions = baseDefinitions;

vi.mock("$lib/stores/soundscape.svelte", () => ({
  getSoundscapeStore: () => ({
    saving: false,
    savePreset,
    toggleSelection,
    playPreset,
    beginPreview,
    previewSounds,
    endPreview,
    definitions,
    groups: [{ id: "outside", name: "Outside", icon: "lucide:trees", createdAtMs: 1, updatedAtMs: 1, version: 1 }],
    persisted: { activeIds: ["fire"] },
    snapshot: { status: "playing" },
  }),
}));

vi.mock("$lib/stores/music-player.svelte", () => ({ getMusicPlayer: () => player }));

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
  definitions = baseDefinitions;
  player.isPlaying = false;
  vi.clearAllMocks();
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
  return document.body.querySelector<HTMLInputElement>('input[aria-label="Preset name"]')!;
}

function soundTile(name: string): HTMLButtonElement {
  return [...document.body.querySelectorAll<HTMLButtonElement>("button[aria-pressed]")].find((button) => button.textContent?.includes(name))!;
}

function saveButton(): HTMLButtonElement {
  return document.body.querySelector<HTMLButtonElement>('button[data-preset-save]')!;
}

function lastPreview(): readonly string[] | undefined {
  return previewSounds.mock.calls.at(-1)?.[0];
}

describe("Background sound preset editor", () => {
  it("starts a new preset empty instead of copying what is playing", async () => {
    await render();
    expect([...document.body.querySelectorAll('button[aria-pressed="true"]')]).toHaveLength(0);
    expect(lastPreview()).toEqual([]);
  });

  it("previews picked sounds without changing the saved selection and saves them in pick order", async () => {
    const onDone = await render();
    nameField().value = "Deep focus";
    nameField().dispatchEvent(new Event("input", { bubbles: true }));
    soundTile("Café").click();
    soundTile("Porch rain").click();
    await tick();
    expect(soundTile("Café").getAttribute("aria-pressed")).toBe("true");
    expect(lastPreview()).toEqual(["cafe", "rain"]);
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
    expect(lastPreview()).toEqual(["rain", "cafe"]);
    soundTile("Porch rain").click();
    await tick();
    saveButton().click();
    await vi.waitFor(() => expect(savePreset).toHaveBeenCalledTimes(1));
    expect(savePreset.mock.calls[0][0]).toMatchObject({ id: "evening", soundscapeIds: ["cafe"], expectedVersion: 3 });
  });

  it("groups sounds the way the builder does", async () => {
    await render();
    const headings = [...document.body.querySelectorAll("h4")].map((heading) => heading.textContent);
    expect(headings).toEqual(["Outside", "Other sounds"]);
  });

  it("silences the preview when listening is turned off and keeps the draft", async () => {
    await render(evening);
    document.body.querySelector<HTMLButtonElement>('button[role="switch"]')!.click();
    await tick();
    expect(lastPreview()).toEqual([]);
    soundTile("Fireplace").click();
    await tick();
    expect(lastPreview()).toEqual([]);
    expect(soundTile("Fireplace").getAttribute("aria-pressed")).toBe("true");
  });

  it("pauses Music while open and hands playback back when it closes", async () => {
    player.isPlaying = true;
    await render(evening);
    expect(player.pausePlayback).toHaveBeenCalledWith("system");
    expect(beginPreview).toHaveBeenCalledOnce();
    await unmount(component!);
    component = undefined;
    expect(endPreview).toHaveBeenCalledOnce();
    expect(player.playPlayback).toHaveBeenCalledWith("system");
  });

  it("leaves Music alone when nothing was playing", async () => {
    await render();
    await unmount(component!);
    component = undefined;
    expect(player.pausePlayback).not.toHaveBeenCalled();
    expect(player.playPlayback).not.toHaveBeenCalled();
    expect(endPreview).toHaveBeenCalledOnce();
  });

  it("explains the sound limit only once it is reached", async () => {
    definitions = Array.from({ length: 17 }, (_, index) => sound(`sound-${index}`, `Sound ${index}`));
    await render({ ...evening, soundscapeIds: definitions.slice(0, 15).map((entry) => entry.id) });
    expect(document.body.querySelector('[role="status"]')).toBeNull();
    soundTile("Sound 15").click();
    await tick();
    expect(document.body.querySelector('[role="status"]')?.textContent).toContain("up to 16 sounds");
    expect(soundTile("Sound 16").disabled).toBe(true);
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
