// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { completeMusicAssignmentDrafts, persistedMusicAssignmentDrafts, updateMusicAssignmentDraft } from "$lib/music/assignment-draft";
import type { MusicContextAssignmentDraft } from "$lib/music/context-assignment";
import type { MusicPlaylistSummary } from "$lib/music/library/contracts";
import ProjectSettingsPlaylistRows from "./ProjectSettingsPlaylistRows.svelte";

vi.mock("$lib/stores/soundscape.svelte", () => {
  const rain = {
    id: "rain", sourceKind: "local-loop", generatedKind: null, bundledIdentity: null, name: "Rain",
    icon: "lucide:audio-lines", groupId: null, availability: "available", localPath: "/rain.ogg",
    createdAtMs: 1, updatedAtMs: 1, version: 1,
  };
  const store = {
    definitions: [rain],
    groups: [],
    presets: [{ id: "evening", name: "Evening", icon: "lucide:moon", soundscapeIds: ["rain"], createdAtMs: 1, updatedAtMs: 1, version: 1 }],
    loading: false,
    ensureCatalog: async () => undefined,
    presetIsPlayable: () => true,
  };
  return { getSoundscapeStore: () => store };
});

const playlist: MusicPlaylistSummary = {
  id: "reading", name: "Reading", icon: "music", shuffleEnabled: false,
  mixEnabled: false, repeatMode: "off", intendedUses: [], sortOrder: 0,
  totalCount: 2, eligibleCount: 2, unavailableCount: 0, snoozedCount: 0,
  localCount: 2, onlineCount: 0, version: 1,
};

let component: ReturnType<typeof mount> | undefined;
let target: HTMLDivElement;

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  target?.remove();
});

/** Mount the three playlist rows inside the settings floating boundary. */
async function render(
  assignments: readonly MusicContextAssignmentDraft[] = [],
  options: { disabled?: boolean; loadingPlaylists?: boolean } = {},
) {
  target = document.createElement("div");
  target.dataset.floatingRoot = "";
  document.body.append(target);
  const onChange = vi.fn<(assignments: MusicContextAssignmentDraft[]) => void>();
  component = mount(ProjectSettingsPlaylistRows, {
    target,
    props: { assignments, playlists: [playlist], onChange, ...options },
  });
  await tick();
  return onChange;
}

/** Open the picker for a named phase without leaving the settings boundary. */
async function open(label: string): Promise<HTMLButtonElement> {
  const trigger = target.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!;
  trigger.click();
  await vi.waitFor(() => expect(target.querySelector('[role="dialog"]')).not.toBeNull());
  return trigger;
}

/** Choose a playlist or None from the open picker. */
async function choose(label: string, optionLabel: string): Promise<void> {
  await open(label);
  const option = [...target.querySelectorAll<HTMLButtonElement>('[role="option"]')]
    .find((button) => button.textContent?.trim().startsWith(optionLabel));
  if (!option) throw new Error(`Missing playlist choice: ${optionLabel}`);
  option.click();
  await tick();
}

describe("Project playlist rows", () => {
  it.each([
    ["Focus playlist", "focus"],
    ["Short break playlist", "short-break"],
    ["Long break playlist", "long-break"],
  ] as const)("selects automatic music from the %s row and saves empty phases as silence", async (label, phase) => {
    const onChange = await render();
    expect([...target.querySelectorAll<HTMLButtonElement>('[aria-haspopup][aria-label$="playlist"]')].map((button) => button.getAttribute("aria-label")))
      .toEqual(["Focus playlist", "Short break playlist", "Long break playlist"]);
    await choose(label, "Reading");
    const next = onChange.mock.calls[0][0];
    expect(next.find((assignment) => assignment.phase === phase)).toMatchObject({
      playlistId: playlist.id, behavior: "play-automatically", soundscapeId: null, soundscapePresetId: null, soundscapeBehavior: "inherit",
    });
    expect(next.filter((assignment) => assignment.phase !== phase).every((assignment) => assignment.behavior === "pause-music" && assignment.playlistId === null)).toBe(true);
    expect(persistedMusicAssignmentDrafts(next)).toHaveLength(3);
  });

  it("clears a phase with None while keeping the other selected playlists", async () => {
    const assignments = completeMusicAssignmentDrafts([]).map((assignment): MusicContextAssignmentDraft => ({
      ...assignment, behavior: "play-automatically", playlistId: playlist.id,
    }));
    const onChange = await render(assignments);
    await choose("Short break playlist", "None");
    expect(onChange.mock.calls[0][0].map((assignment) => [assignment.behavior, assignment.playlistId])).toEqual([
      ["play-automatically", playlist.id], ["pause-music", null], ["play-automatically", playlist.id],
    ]);
    expect(assignments[1].playlistId).toBe(playlist.id);
  });

  it("replaces legacy playback with automatic playback while keeping each phase's background sound", async () => {
    const assignments = updateMusicAssignmentDraft([], "focus", {
      behavior: "prepare-silently", playlistId: playlist.id,
      soundscapeBehavior: "play-selected", soundscapeId: "rain",
    });
    const onChange = await render(assignments);
    await choose("Long break playlist", "Reading");
    const next = onChange.mock.calls[0][0];
    expect(next[0]).toMatchObject({ behavior: "play-automatically", playlistId: playlist.id, soundscapeBehavior: "play-selected", soundscapeId: "rain" });
    expect(next.slice(1).every((assignment) => assignment.soundscapeBehavior === "inherit")).toBe(true);
  });

  it("chooses a background preset or silence per phase without touching music", async () => {
    const onChange = await render(updateMusicAssignmentDraft([], "short-break", { behavior: "pause-music" }));
    expect(target.querySelector('[aria-label="Short break background sound"]')?.textContent).toContain("Don't change");
    await choose("Short break background sound", "Evening");
    expect(onChange.mock.calls[0][0][1]).toMatchObject({
      behavior: "pause-music", soundscapeBehavior: "play-selected", soundscapeId: null, soundscapePresetId: "evening",
    });
    await choose("Focus background sound", "Silence");
    expect(onChange.mock.calls[1][0][0]).toMatchObject({ behavior: "inherit", soundscapeBehavior: "pause-soundscape", soundscapePresetId: null });
    expect(persistedMusicAssignmentDrafts(onChange.mock.calls[1][0]).map((assignment) => assignment.phase)).toEqual(["focus", "short-break"]);
  });

  it("flags a deleted background sound so the phase can be repaired", async () => {
    await render(updateMusicAssignmentDraft([], "long-break", { soundscapeBehavior: "play-selected", soundscapeId: "deleted" }));
    expect(target.querySelector('[aria-label="Long break background sound"]')?.textContent).toContain("Selected sound is unavailable");
  });

  it("keeps an unavailable playlist explicit and lets the picker repair it", async () => {
    const onChange = await render(updateMusicAssignmentDraft([], "focus", {
      behavior: "play-automatically", playlistId: "deleted-playlist",
    }));
    expect(target.querySelector('[aria-label="Focus playlist"]')?.textContent).toContain("Selected playlist is unavailable");
    await choose("Focus playlist", "Reading");
    expect(onChange.mock.calls[0][0][0]).toMatchObject({ behavior: "play-automatically", playlistId: playlist.id });
  });

  it("searches playlists, consumes menu Escape, and restores focus to the invoking row", async () => {
    const onChange = await render();
    const parentKeydown = vi.fn();
    target.addEventListener("keydown", parentKeydown);
    const trigger = await open("Long break playlist");
    const menu = target.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(menu.parentElement).toBe(target);
    const search = menu.querySelector<HTMLInputElement>("input")!;
    await vi.waitFor(() => expect(document.activeElement).toBe(search));
    search.value = "read";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    expect(menu.querySelectorAll('[role="option"]')).toHaveLength(1);
    search.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(menu.querySelector('[role="option"]'));
    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(document.activeElement).toBe(trigger));
    expect(target.querySelector('[role="dialog"]')).toBeNull();
    expect(parentKeydown).not.toHaveBeenCalled();
    expect(onChange).not.toHaveBeenCalled();
  });

  it.each([{ disabled: true }, { loadingPlaylists: true }])("locks all three playlist rows during $disabled disabled or $loadingPlaylists loading state", async (options) => {
    const onChange = await render([], options);
    const triggers = [...target.querySelectorAll<HTMLButtonElement>('[aria-haspopup][aria-label$="playlist"]')];
    expect(triggers).toHaveLength(3);
    expect(triggers.every((button) => button.disabled)).toBe(true);
    triggers[0].click();
    expect(target.querySelector('[role="dialog"]')).toBeNull();
    expect(onChange).not.toHaveBeenCalled();
  });
});
