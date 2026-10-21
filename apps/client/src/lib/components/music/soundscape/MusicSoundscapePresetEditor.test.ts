// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { DEFAULT_SOUNDSCAPE_PRESET_LEVEL, type MusicSoundscapeDefinition, type MusicSoundscapePreset, type MusicSoundscapePresetWrite } from "$lib/music/soundscape/contracts";
import type { SoundscapeMix } from "$lib/stores/soundscape.svelte";
import MusicSoundscapePresetEditor from "./MusicSoundscapePresetEditor.svelte";

const savePreset = vi.fn(async (write: MusicSoundscapePresetWrite): Promise<MusicSoundscapePreset> => ({
  id: write.id, name: write.name, icon: write.icon, level: write.level, sounds: write.sounds,
  createdAtMs: 1, updatedAtMs: write.updatedAtMs, version: (write.expectedVersion ?? 0) + 1,
}));
const playPreset = vi.fn(async () => undefined);
const setPresetLevel = vi.fn(async () => undefined);
const beginPreview = vi.fn(async () => undefined);
const previewMix = vi.fn(async (_mix: SoundscapeMix | null) => undefined);
const endPreview = vi.fn(async () => undefined);
const player = {
  isPlaying: false,
  pausePlayback: vi.fn(async () => { player.isPlaying = false; }),
  playPlayback: vi.fn(async () => { player.isPlaying = true; }),
};

function sound(id: string, name: string): MusicSoundscapeDefinition {
  return {
    id, sourceKind: "local-loop", generatedKind: null, bundledIdentity: null, name, icon: "lucide:audio-lines",
    availability: "available", localPath: `/sounds/${id}.ogg`, createdAtMs: 1, updatedAtMs: 1, version: 1,
  };
}

const baseDefinitions = [sound("rain", "Porch rain"), sound("cafe", "Café"), sound("fire", "Fireplace")];
let definitions = baseDefinitions;
let presets: MusicSoundscapePreset[] = [];

vi.mock("$lib/stores/soundscape.svelte", () => ({
  getSoundscapeStore: () => ({
    saving: false,
    savePreset,
    playPreset,
    setPresetLevel,
    beginPreview,
    previewMix,
    endPreview,
    definitions,
    presets,
    persisted: { activePresetId: "other" },
    snapshot: { status: "playing" },
  }),
}));

vi.mock("$lib/stores/music-player.svelte", () => ({ getMusicPlayer: () => player }));

const evening: MusicSoundscapePreset = {
  id: "evening", name: "Evening", icon: "lucide:moon", level: 0.8,
  sounds: [{ soundscapeId: "rain", level: 1 }, { soundscapeId: "cafe", level: 0.6 }],
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
  presets = [];
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
  return document.body.querySelector<HTMLInputElement>('input[aria-label="Soundscape name"]')!;
}

function soundTile(name: string): HTMLButtonElement {
  return [...document.body.querySelectorAll<HTMLButtonElement>("button[aria-pressed]")].find((button) => button.textContent?.includes(name))!;
}

function slider(label: string): HTMLInputElement {
  return document.body.querySelector<HTMLInputElement>(`input[type="range"][aria-label="${label}"]`)!;
}

function slide(label: string, value: number): void {
  const input = slider(label);
  input.value = String(value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

function saveButton(): HTMLButtonElement {
  return document.body.querySelector<HTMLButtonElement>("button[data-preset-save]")!;
}

function lastPreview(): SoundscapeMix | null | undefined {
  return previewMix.mock.calls.at(-1)?.[0];
}

describe("Soundscape preset editor", () => {
  it("starts a new preset empty at the default level instead of copying what is playing", async () => {
    await render();
    expect([...document.body.querySelectorAll('button[aria-pressed="true"]')]).toHaveLength(0);
    expect(lastPreview()).toEqual({ sounds: [], level: DEFAULT_SOUNDSCAPE_PRESET_LEVEL });
    expect(slider("Soundscape level").value).toBe("0");
  });

  it("previews picked sounds at full mix level and saves them in pick order", async () => {
    const onDone = await render();
    nameField().value = "Deep focus";
    nameField().dispatchEvent(new Event("input", { bubbles: true }));
    soundTile("Café").click();
    soundTile("Porch rain").click();
    await tick();
    expect(soundTile("Café").getAttribute("aria-pressed")).toBe("true");
    expect(lastPreview()).toEqual({ sounds: [{ soundscapeId: "cafe", level: 1 }, { soundscapeId: "rain", level: 1 }], level: DEFAULT_SOUNDSCAPE_PRESET_LEVEL });
    expect(playPreset).not.toHaveBeenCalled();
    saveButton().click();
    await vi.waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(savePreset).toHaveBeenCalledWith(expect.objectContaining({
      name: "Deep focus", level: DEFAULT_SOUNDSCAPE_PRESET_LEVEL, sounds: [{ soundscapeId: "cafe", level: 1 }, { soundscapeId: "rain", level: 1 }], expectedVersion: null,
    }));
  });

  it("edits a preset from its own sounds, levels, and version", async () => {
    await render(evening);
    expect(nameField().value).toBe("Evening");
    expect(soundTile("Porch rain").getAttribute("aria-pressed")).toBe("true");
    expect(soundTile("Fireplace").getAttribute("aria-pressed")).toBe("false");
    expect(slider("Café level").value).toBe("60");
    expect(slider("Soundscape level").value).toBe("-20");
    expect(lastPreview()).toEqual({ sounds: evening.sounds, level: 0.8 });
    soundTile("Porch rain").click();
    await tick();
    saveButton().click();
    await vi.waitFor(() => expect(savePreset).toHaveBeenCalledTimes(1));
    expect(savePreset.mock.calls[0][0]).toMatchObject({ id: "evening", sounds: [{ soundscapeId: "cafe", level: 0.6 }], level: 0.8, expectedVersion: 3 });
  });

  it("saves against the latest stored version after the level was moved elsewhere", async () => {
    presets = [{ ...evening, version: 5 }];
    await render(evening);
    saveButton().click();
    await vi.waitFor(() => expect(savePreset).toHaveBeenCalledTimes(1));
    expect(savePreset.mock.calls[0][0]).toMatchObject({ expectedVersion: 5 });
  });

  it("previews and saves the preset level and each sound's share of it", async () => {
    await render(evening);
    slide("Soundscape level", 30);
    slide("Café level", 25);
    await tick();
    expect(lastPreview()).toEqual({ sounds: [{ soundscapeId: "rain", level: 1 }, { soundscapeId: "cafe", level: 0.25 }], level: 1.3 });
    expect(setPresetLevel).not.toHaveBeenCalled();
    saveButton().click();
    await vi.waitFor(() => expect(savePreset).toHaveBeenCalledTimes(1));
    expect(savePreset.mock.calls[0][0]).toMatchObject({ level: 1.3, sounds: [{ soundscapeId: "rain", level: 1 }, { soundscapeId: "cafe", level: 0.25 }] });
  });

  it("shows a level row only for picked sounds", async () => {
    await render(evening);
    expect(slider("Fireplace level")).toBeNull();
    soundTile("Fireplace").click();
    await tick();
    expect(slider("Fireplace level").value).toBe("100");
  });

  it("lists generated noise before local layers in one list", async () => {
    const brown: MusicSoundscapeDefinition = { ...sound("generated-brown-noise", "Brown noise"), sourceKind: "generated-noise", generatedKind: "brown", localPath: null };
    definitions = [...baseDefinitions, brown];
    await render();
    const names = [...document.body.querySelectorAll<HTMLButtonElement>("button[aria-pressed]")].map((button) => button.textContent?.trim());
    expect(names).toEqual(["Brown noise", "Porch rain", "Café", "Fireplace"]);
    expect(document.body.querySelectorAll("h4")).toHaveLength(0);
  });

  it("silences the preview when listening is turned off and keeps the draft", async () => {
    await render(evening);
    document.body.querySelector<HTMLButtonElement>('button[role="switch"]')!.click();
    await tick();
    expect(lastPreview()).toBeNull();
    soundTile("Fireplace").click();
    await tick();
    expect(lastPreview()).toBeNull();
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
    await render({ ...evening, sounds: definitions.slice(0, 15).map((entry) => ({ soundscapeId: entry.id, level: 1 })) });
    expect(document.body.querySelector('[role="status"]')).toBeNull();
    soundTile("Sound 15").click();
    await tick();
    expect(document.body.querySelector('[role="status"]')?.textContent).toContain("up to 16 layers");
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
