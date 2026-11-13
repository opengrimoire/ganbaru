// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatAssignmentBudgetInput, ChatAssignmentBudgetRead } from "$lib/chat/contracts";
import type { BudgetMeterKind } from "$lib/chat/budgets/budget-model";
import ChatAssignmentBudgetBar from "./ChatAssignmentBudgetBar.svelte";

function budget(overrides: Partial<ChatAssignmentBudgetRead> = {}): ChatAssignmentBudgetRead {
  return {
    assignmentId: "assignment:a",
    tokenLimit: 20_000,
    workingSecondsLimit: null,
    setByParticipantId: "participant:victor",
    setAt: "2026-11-12T10:00:00.000Z",
    usedCountedTokens: 0,
    usedWorkingSeconds: 0,
    ...overrides,
  };
}

async function settle(): Promise<void> {
  for (let index = 0; index < 4; index += 1) {
    await Promise.resolve();
    await tick();
  }
}

function input(element: HTMLInputElement, value: string): void {
  element.value = value;
  element.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
}

describe("ChatAssignmentBudgetBar", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
  });

  function render(props: {
    budget: ChatAssignmentBudgetRead;
    stoppedAt?: BudgetMeterKind | null;
    onSaveLimits?: (budget: ChatAssignmentBudgetInput | null) => Promise<void>;
    onRetry?: () => unknown;
  }): HTMLElement {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatAssignmentBudgetBar, {
      target,
      props: {
        teammateName: "Ada",
        onSaveLimits: vi.fn<(budget: ChatAssignmentBudgetInput | null) => Promise<void>>().mockResolvedValue(),
        onRetry: vi.fn(),
        ...props,
      },
    });
    flushSync();
    const bar = target.querySelector<HTMLElement>("[data-assignment-budget]");
    if (!bar) throw new Error("The budget bar renders");
    return bar;
  }

  it("shows used and limit for each set limit without a notice below 75 percent", () => {
    const bar = render({ budget: budget({ workingSecondsLimit: 1_800, usedCountedTokens: 5_000, usedWorkingSeconds: 600 }) });

    const meters = [...bar.querySelectorAll<HTMLElement>("[data-budget-meter]")];
    expect(meters.map((meter) => meter.dataset.budgetMeter)).toEqual(["tokens", "workingTime"]);
    expect(meters[0]?.textContent).toContain("5K of 20K");
    expect(meters[1]?.textContent).toContain("10 min of 30 min");
    expect(bar.querySelector("[role='status']")).toBeNull();
  });

  it("warns at 75 percent of the token limit", () => {
    const bar = render({ budget: budget({ usedCountedTokens: 15_000 }) });

    expect(bar.querySelector("[role='status']")?.textContent)
      .toContain("Ada has used 75 percent of this assignment's token limit");
  });

  it("warns at 90 percent of the closest limit", () => {
    const bar = render({ budget: budget({ workingSecondsLimit: 600, usedCountedTokens: 15_000, usedWorkingSeconds: 545 }) });

    expect(bar.querySelector("[role='status']")?.textContent)
      .toContain("Ada has used 90 percent of this assignment's working time limit");
  });

  it("names the reached limit and retries", () => {
    const onRetry = vi.fn();
    const bar = render({ budget: budget({ usedCountedTokens: 20_400 }), stoppedAt: "tokens", onRetry });

    expect(bar.textContent).toContain("Ada stopped at the token limit set for this work");
    expect(bar.textContent).not.toContain("percent");
    bar.querySelector<HTMLButtonElement>("button[aria-label='Retry work']")?.click();
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it("raises the limit from the reached state", async () => {
    const onSaveLimits = vi.fn<(budget: ChatAssignmentBudgetInput | null) => Promise<void>>().mockResolvedValue();
    const bar = render({ budget: budget({ usedCountedTokens: 20_400 }), stoppedAt: "tokens", onSaveLimits });

    [...bar.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Raise limit")?.click();
    flushSync();
    const [tokens, minutes] = [...bar.querySelectorAll<HTMLInputElement>("form input")];
    if (!tokens || !minutes) throw new Error("The limit fields render");
    expect(tokens.value).toBe("20000");
    expect(minutes.value).toBe("");

    input(tokens, "40,000");
    input(minutes, "45");
    bar.querySelector<HTMLFormElement>("form")?.requestSubmit();
    await settle();

    expect(onSaveLimits).toHaveBeenCalledWith({ tokenLimit: 40_000, workingSecondsLimit: 2_700 });
    expect(bar.querySelector("form")).toBeNull();
  });

  it("refuses limits outside the backend bounds and removes the budget when both fields are empty", async () => {
    const onSaveLimits = vi.fn<(budget: ChatAssignmentBudgetInput | null) => Promise<void>>().mockResolvedValue();
    const bar = render({ budget: budget({ usedCountedTokens: 20_400 }), stoppedAt: "tokens", onSaveLimits });

    [...bar.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Raise limit")?.click();
    flushSync();
    const tokens = bar.querySelector<HTMLInputElement>("form input");
    if (!tokens) throw new Error("The token field renders");
    input(tokens, "500");
    expect(bar.querySelector("form [role='alert']")?.textContent).toContain("Enter a token limit from 1,000 to");
    expect(bar.querySelector<HTMLButtonElement>("form button[type='submit']")?.disabled).toBe(true);

    input(tokens, "");
    bar.querySelector<HTMLFormElement>("form")?.requestSubmit();
    await settle();
    expect(onSaveLimits).toHaveBeenCalledWith(null);
  });

  it("keeps the form open and shows the error when saving fails", async () => {
    const onSaveLimits = vi.fn<(budget: ChatAssignmentBudgetInput | null) => Promise<void>>()
      .mockRejectedValue(new Error("offline"));
    const bar = render({ budget: budget({ usedCountedTokens: 20_400 }), stoppedAt: "tokens", onSaveLimits });

    [...bar.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Raise limit")?.click();
    flushSync();
    bar.querySelector<HTMLFormElement>("form")?.requestSubmit();
    await settle();

    expect(bar.querySelector("form")).not.toBeNull();
    expect(bar.querySelector("form [role='alert']")).not.toBeNull();
  });
});
