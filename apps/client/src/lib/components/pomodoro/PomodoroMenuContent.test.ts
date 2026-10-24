// @vitest-environment jsdom

import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { FocusOtherDeviceActivity } from "$lib/pomodoro/device-activity";
import PomodoroMenuContent from "./PomodoroMenuContent.svelte";

const laptopFocusing: FocusOtherDeviceActivity = {
  device: { deviceId: "laptop", deviceLabel: "Laptop", ownDevice: false },
  state: "focus",
  occurrenceId: "occurrence",
  eventTitle: "Write",
  phaseStartedAtMs: 1_000,
  phaseEndsAtMs: 2_000,
  expiresAtMs: 3_000,
  updatedAtMs: 1_500,
};

const pomodoro = vi.hoisted(() => ({
  isActive: true,
  isRunning: true,
  phase: "focus" as const,
  canPauseResume: true,
  canAddFocusTime: true,
  formattedTime: "24:30",
  pause: vi.fn(),
  start: vi.fn(),
  addFocusTime: vi.fn(),
  skip: vi.fn(),
  startScheduledSession: vi.fn(async (): Promise<void> => {}),
}));

const musicPlayer = vi.hoisted(() => ({
  currentSource: null,
  loadedTitle: "",
  snapshot: { status: "idle" as const },
  isBusy: false,
  isPlaying: false,
  volumeMax: 1,
  volumeControlValue: 1,
  contextPlayback: null,
  canPlayPreviousTrack: false,
  canPlayNextTrack: false,
  setVolume: vi.fn(async () => undefined),
  togglePlay: vi.fn(async () => undefined),
  playPreviousTrack: vi.fn(async () => undefined),
  playNextTrack: vi.fn(async () => undefined),
  inspectContextAssignment: vi.fn(),
}));

const focusActivity = vi.hoisted(() => ({
  read: vi.fn(async (): Promise<FocusOtherDeviceActivity[]> => []),
  applied: [] as ((tables: string[]) => void)[],
}));

vi.mock("$lib/api/focus", () => ({
  readFocusDeviceActivity: focusActivity.read,
}));

vi.mock("$lib/api/sync", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/sync")>(),
  listenSyncApplied: vi.fn(async (onApplied: (tables: string[]) => void) => {
    focusActivity.applied.push(onApplied);
    return () => {};
  }),
}));

vi.mock("$lib/stores/pomodoro.svelte", () => ({
  getPomodoro: () => pomodoro,
}));

vi.mock("$lib/stores/music-player.svelte", () => ({
  getMusicPlayer: () => musicPlayer,
}));

describe("PomodoroMenuContent", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    target = undefined;
    component = undefined;
    pomodoro.pause.mockClear();
    pomodoro.startScheduledSession.mockReset();
    pomodoro.isActive = true;
    focusActivity.read.mockReset();
    focusActivity.read.mockResolvedValue([]);
    focusActivity.applied.length = 0;
  });

  function mountMenu(onDismiss = vi.fn()): HTMLDivElement {
    const host = document.createElement("div");
    document.body.append(host);
    target = host;
    component = mount(PomodoroMenuContent, { target: host, props: { includeMusic: false, onDismiss, onOpenMusic: vi.fn() } });
    return host;
  }

  function activityLines(host: HTMLElement): string[] {
    return [...host.querySelectorAll("[data-focus-activity]")].map((line) => line.textContent.trim());
  }

  it("shows what other devices are focusing on and follows applied Focus notices", async () => {
    focusActivity.read.mockResolvedValueOnce([laptopFocusing]);
    const host = mountMenu();
    await vi.waitFor(() => expect(activityLines(host)).toEqual(["Laptop is focusing on Write"]));

    focusActivity.read.mockResolvedValueOnce([]);
    for (const onApplied of focusActivity.applied) onApplied(["calendar_events"]);
    expect(focusActivity.read).toHaveBeenCalledOnce();
    for (const onApplied of focusActivity.applied) onApplied(["focus"]);
    await vi.waitFor(() => expect(activityLines(host)).toEqual([]));
    expect(focusActivity.read).toHaveBeenCalledTimes(2);
  });

  it("asks before an explicit start while another device is focusing", async () => {
    pomodoro.isActive = false;
    focusActivity.read.mockResolvedValueOnce([laptopFocusing]);
    const onDismiss = vi.fn();
    const host = mountMenu(onDismiss);
    await vi.waitFor(() => expect(activityLines(host)).toHaveLength(1));

    host.querySelector<HTMLButtonElement>("button")?.click();
    await vi.waitFor(() => expect(host.textContent).toContain("Start here as well?"));
    expect(pomodoro.startScheduledSession).not.toHaveBeenCalled();
    expect(activityLines(host)).toEqual(["Laptop is focusing on Write"]);

    const cancel = [...host.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent.includes("Cancel"));
    cancel?.click();
    await vi.waitFor(() => expect(host.textContent).not.toContain("Start here as well?"));

    host.querySelector<HTMLButtonElement>("button")?.click();
    await vi.waitFor(() => expect(host.textContent).toContain("Start anyway"));
    const startAnyway = [...host.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent.includes("Start anyway"));
    startAnyway?.click();
    expect(pomodoro.startScheduledSession).toHaveBeenCalledOnce();
    await vi.waitFor(() => expect(onDismiss).toHaveBeenCalledOnce());
  });

  it("uses the shared Pomodoro actions in touch presentation", () => {
    target = document.createElement("div");
    document.body.append(target);
    const onDismiss = vi.fn();
    component = mount(PomodoroMenuContent, {
      target,
      props: {
        includeMusic: false,
        touch: true,
        onDismiss,
        onOpenMusic: vi.fn(),
      },
    });

    const actions = target.querySelectorAll<HTMLButtonElement>("button");
    expect(actions).toHaveLength(3);
    expect(actions.item(0).classList.contains("menu-item")).toBe(true);
    actions.item(0).click();
    expect(pomodoro.pause).toHaveBeenCalledOnce();
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it("waits for native admission before dismissing an explicit-start menu", async () => {
    pomodoro.isActive = false;
    let accept: (() => void) | undefined;
    pomodoro.startScheduledSession.mockImplementationOnce(() => new Promise<void>((resolve) => { accept = resolve; }));
    target = document.createElement("div");
    document.body.append(target);
    const onDismiss = vi.fn();
    component = mount(PomodoroMenuContent, { target, props: { includeMusic: false, onDismiss, onOpenMusic: vi.fn() } });
    target.querySelector<HTMLButtonElement>("button")?.click();
    expect(pomodoro.startScheduledSession).toHaveBeenCalledOnce();
    expect(onDismiss).not.toHaveBeenCalled();
    accept?.();
    await vi.waitFor(() => expect(onDismiss).toHaveBeenCalledOnce());
  });
});
