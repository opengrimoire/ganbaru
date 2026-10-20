// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MusicSoundscapeDefinition, MusicSoundscapePreset, MusicSoundscapePresetWrite } from "$lib/music/soundscape/contracts";
import MusicSoundscapePresetForm from "./MusicSoundscapePresetForm.svelte";

const savePreset = vi.fn(async (write: MusicSoundscapePresetWrite): Promise<MusicSoundscapePreset> => ({
  id: write.id, name: write.name, icon: write.icon, soundscapeIds: write.soundscapeIds,
  createdAtMs: 1, updatedAtMs: write.updatedAtMs, version: (write.expectedVersion ?? 0) + 1,
}));

function sound(id: string, name: string): MusicSoundscapeDefinition {
  return {
    id, sourceKind: "local-loop", generatedKind: null, bundledIdentity: null, name, icon: "lucide:audio-lines",
    groupId: null, availability: "available", localPath: `/sounds/${id}.ogg`, createdAtMs: 1, updatedAtMs: 1, version: 1,
  };
}

const definitions = [sound("rain", "Porch rain"), sound("cafe", "Café")];

vi.mock("$lib/stores/soundscape.svelte", () => ({
  getSoundscapeStore: () => ({ saving: false, savePreset, definitions }),
}));

let component: ReturnType<typeof mount> | undefined;
let target: HTMLDivElement;

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  target?.remove();
  savePreset.mockClear();
});

async function render(soundIds: string[]) {
  target = document.createElement("div");
  document.body.append(target);
  const onDone = vi.fn<(preset: MusicSoundscapePreset | null) => void>();
  component = mount(MusicSoundscapePresetForm, { target, props: { soundIds, onDone } });
  await tick();
  return onDone;
}

function nameField(): HTMLInputElement {
  return target.querySelector<HTMLInputElement>('input[aria-label="Preset name"]')!;
}

async function typeName(value: string): Promise<void> {
  const input = nameField();
  input.value = value;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  await tick();
}

function saveButton(): HTMLButtonElement {
  return target.querySelector<HTMLButtonElement>('button[type="submit"]')!;
}

describe("Background sound preset quick save", () => {
  it("names the sounds it will save before saving them with a trimmed name", async () => {
    const onDone = await render(["rain", "cafe"]);
    expect(document.activeElement).toBe(nameField());
    expect(target.textContent).toContain("Includes Porch rain and Café");
    expect(saveButton().disabled).toBe(true);
    await typeName("  Deep focus  ");
    saveButton().click();
    await vi.waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(savePreset).toHaveBeenCalledWith(expect.objectContaining({
      name: "Deep focus", soundscapeIds: ["rain", "cafe"], expectedVersion: null, icon: "lucide:layers",
    }));
    expect(onDone.mock.calls[0][0]?.name).toBe("Deep focus");
  });

  it("cannot save without sounds", async () => {
    await render([]);
    await typeName("Empty");
    expect(saveButton().disabled).toBe(true);
  });

  it("cancels with Escape from the name field without saving", async () => {
    const onDone = await render(["rain"]);
    nameField().dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    expect(onDone).toHaveBeenCalledWith(null);
    expect(savePreset).not.toHaveBeenCalled();
  });
});
