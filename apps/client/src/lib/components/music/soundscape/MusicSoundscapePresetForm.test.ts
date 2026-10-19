// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MusicSoundscapePreset, MusicSoundscapePresetWrite } from "$lib/music/soundscape/contracts";
import MusicSoundscapePresetForm from "./MusicSoundscapePresetForm.svelte";

const savePreset = vi.fn(async (write: MusicSoundscapePresetWrite): Promise<MusicSoundscapePreset> => ({
  id: write.id, name: write.name, icon: write.icon, soundscapeIds: write.soundscapeIds,
  createdAtMs: 1, updatedAtMs: write.updatedAtMs, version: (write.expectedVersion ?? 0) + 1,
}));

vi.mock("$lib/stores/soundscape.svelte", () => ({
  getSoundscapeStore: () => ({ saving: false, savePreset }),
}));

const evening: MusicSoundscapePreset = {
  id: "evening", name: "Evening", icon: "lucide:moon", soundscapeIds: ["rain", "fire"],
  createdAtMs: 1, updatedAtMs: 1, version: 3,
};

let component: ReturnType<typeof mount> | undefined;
let target: HTMLDivElement;

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  target?.remove();
  savePreset.mockClear();
});

async function render(props: { preset?: MusicSoundscapePreset | null; currentIds: string[] }) {
  target = document.createElement("div");
  document.body.append(target);
  const onDone = vi.fn<(preset: MusicSoundscapePreset | null) => void>();
  component = mount(MusicSoundscapePresetForm, { target, props: { ...props, onDone } });
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

describe("Background sound preset form", () => {
  it("saves the sounds playing now as a new preset with a trimmed name", async () => {
    const onDone = await render({ currentIds: ["rain", "cafe"] });
    expect(document.activeElement).toBe(nameField());
    expect(saveButton().disabled).toBe(true);
    await typeName("  Deep focus  ");
    saveButton().click();
    await vi.waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(savePreset).toHaveBeenCalledWith(expect.objectContaining({
      name: "Deep focus", soundscapeIds: ["rain", "cafe"], expectedVersion: null, icon: "lucide:layers",
    }));
    expect(onDone.mock.calls[0][0]?.name).toBe("Deep focus");
  });

  it("cannot save a new preset when nothing is selected", async () => {
    await render({ currentIds: [] });
    await typeName("Empty");
    expect(saveButton().disabled).toBe(true);
  });

  it("keeps an edited preset's sounds unless the current selection is chosen", async () => {
    await render({ preset: evening, currentIds: ["cafe"] });
    expect(nameField().value).toBe("Evening");
    await typeName("Night");
    saveButton().click();
    await vi.waitFor(() => expect(savePreset).toHaveBeenCalledTimes(1));
    expect(savePreset.mock.calls[0][0]).toMatchObject({ id: "evening", name: "Night", soundscapeIds: ["rain", "fire"], expectedVersion: 3 });

    target.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click();
    await tick();
    saveButton().click();
    await vi.waitFor(() => expect(savePreset).toHaveBeenCalledTimes(2));
    expect(savePreset.mock.calls[1][0].soundscapeIds).toEqual(["cafe"]);
  });

  it("hides the current selection option when it already matches the preset", async () => {
    await render({ preset: evening, currentIds: ["fire", "rain"] });
    expect(target.querySelector('input[type="checkbox"]')).toBeNull();
  });

  it("cancels with Escape from the name field without saving", async () => {
    const onDone = await render({ currentIds: ["rain"] });
    nameField().dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    expect(onDone).toHaveBeenCalledWith(null);
    expect(savePreset).not.toHaveBeenCalled();
  });
});
