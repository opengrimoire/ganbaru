// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Temporal } from "@js-temporal/polyfill";
import type { ChatRecurrenceSpec } from "$lib/chat/contracts";
import ChatMessageScheduleMenu from "./ChatMessageScheduleMenu.svelte";

type SelectSpy = (scheduledFor: string, recurrence: ChatRecurrenceSpec | null) => void;

function futureInstant(days: number): string {
  const start = Temporal.Now.zonedDateTimeISO().add({ days }).with({ hour: 9, minute: 0, second: 0, millisecond: 0, microsecond: 0, nanosecond: 0 });
  return start.toInstant().toString();
}

describe("ChatMessageScheduleMenu", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    vi.restoreAllMocks();
  });

  function render(props: Record<string, unknown>): { onSelect: ReturnType<typeof vi.fn<SelectSpy>>; onClose: ReturnType<typeof vi.fn<() => void>> } {
    const onSelect = vi.fn<SelectSpy>();
    const onClose = vi.fn<() => void>();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatMessageScheduleMenu, { target, props: { mode: "choose", onSelect, onClose, ...props } });
    return { onSelect, onClose };
  }

  async function chooseRepeat(label: string): Promise<void> {
    target?.querySelector<HTMLButtonElement>('.repeat-row button[aria-label="Repeat"]')?.click();
    await tick();
    const option = [...document.body.querySelectorAll<HTMLButtonElement>('[role="option"]')]
      .find((entry) => entry.textContent?.trim() === label);
    option?.click();
    await tick();
  }

  it("stays open after a choice and applies a repeat change to the chosen time at once", async () => {
    const selected = futureInstant(2);
    const { onSelect, onClose } = render({ persistent: true, selectedScheduledFor: selected });
    await tick();

    target?.querySelector<HTMLButtonElement>(".schedule-choices .menu-item")?.click();
    await tick();
    expect(onSelect).toHaveBeenCalledOnce();
    expect(onClose).not.toHaveBeenCalled();

    onSelect.mockClear();
    await chooseRepeat("Every day");
    expect(onSelect).toHaveBeenCalledOnce();
    const [scheduledFor, recurrence] = onSelect.mock.calls[0];
    expect(scheduledFor).toBe(selected);
    expect(recurrence).not.toBeNull();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("closes after a choice and keeps repeat changes for the next choice when not persistent", async () => {
    const { onSelect, onClose } = render({ selectedScheduledFor: futureInstant(2) });
    await tick();

    await chooseRepeat("Every day");
    expect(onSelect).not.toHaveBeenCalled();

    target?.querySelector<HTMLButtonElement>(".schedule-choices .menu-item")?.click();
    await tick();
    expect(onSelect).toHaveBeenCalledOnce();
    expect(onSelect.mock.calls[0][1]).not.toBeNull();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("disables repeating with a reason until a time is chosen", async () => {
    render({ persistent: true, selectedScheduledFor: null });
    await tick();

    const row = target?.querySelector<HTMLElement>(".repeat-row");
    expect(row?.dataset.appTooltip).toBe("Choose when to send it first to make it repeat");
    expect(row?.querySelector<HTMLButtonElement>('button[aria-label="Repeat"]')?.disabled).toBe(true);
  });

  it("marks the day chosen in the custom view", async () => {
    render({ persistent: true, startCustom: true });
    await tick();
    const enabledDays = [...(target?.querySelectorAll<HTMLButtonElement>("button[data-date]:not(:disabled)") ?? [])];
    const day = enabledDays.at(-1)?.dataset.date;
    const button = () => target?.querySelector<HTMLButtonElement>(`button[data-date="${day}"]`);
    expect(button()?.getAttribute("style")).not.toContain("var(--primary)");

    button()?.click();
    await tick();

    expect(button()?.getAttribute("style")).toContain("var(--primary)");
  });
});
