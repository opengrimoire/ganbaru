// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ModelOptionSelection } from "$lib/chat/contracts";
import { getChat } from "$lib/stores/chat.svelte";
import { modelSettings, resetChatComposerTestStore } from "$lib/components/chat/ChatComposer.test-support";
import ChatModelControls from "./ChatModelControls.svelte";

interface ControlledSelection {
  providerInstanceId: string | null;
  modelId: string | null;
  providerManaged: boolean;
  options: ModelOptionSelection[];
}

function domRect(left: number, top: number, width: number, height: number): DOMRect {
  return {
    x: left,
    y: top,
    left,
    top,
    right: left + width,
    bottom: top + height,
    width,
    height,
    toJSON: () => ({}),
  };
}

describe("ChatModelControls controlled mode", () => {
  const mounted: Array<{
    component: ReturnType<typeof mount>;
    target: HTMLDivElement;
  }> = [];

  beforeEach(() => {
    resetChatComposerTestStore();
    getChat().settings = modelSettings();
  });

  afterEach(async () => {
    while (mounted.length > 0) {
      const entry = mounted.pop();
      if (!entry) continue;
      await unmount(entry.component);
      entry.target.remove();
    }
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("emits provider option changes without mutating the live composer", async () => {
    const target = document.createElement("div");
    document.body.append(target);
    const changes: ControlledSelection[] = [];
    const component = mount(ChatModelControls, {
      target,
      props: {
        value: {
          providerInstanceId: "codex-local",
          modelId: "gpt-5.6-sol",
          providerManaged: false,
          options: [
            { key: "reasoning_effort", value: { kind: "choice", value: "medium" } },
            { key: "service_tier", value: { kind: "choice", value: "standard" } },
          ],
        },
        onChange: (selection: ControlledSelection) => changes.push(selection),
      },
    });
    mounted.push({ component, target });
    const composerBefore = getChat().composer.modelSelection;

    target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]")?.click();
    await tick();
    document.querySelector<HTMLButtonElement>(".model-popover .fast-button")?.click();
    await tick();

    expect(changes).toHaveLength(1);
    expect(changes[0]?.options).toContainEqual({
      key: "service_tier",
      value: { kind: "choice", value: "fast" },
    });
    expect(getChat().composer.modelSelection).toBe(composerBefore);
  });

  it("prevents editing when the teammate is archived", async () => {
    const target = document.createElement("div");
    document.body.append(target);
    const component = mount(ChatModelControls, {
      target,
      props: {
        value: {
          providerInstanceId: "codex-local",
          modelId: "gpt-5.6-sol",
          providerManaged: false,
          options: [],
        },
        disabled: true,
      },
    });
    mounted.push({ component, target });

    const trigger = target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]");
    expect(trigger?.disabled).toBe(true);
    trigger?.click();
    await tick();
    expect(document.querySelector(".model-popover")).toBeNull();
  });

  it("opens without resizing the trigger and rotates its chevron", async () => {
    const target = document.createElement("div");
    document.body.append(target);
    const component = mount(ChatModelControls, {
      target,
      props: {
        value: {
          providerInstanceId: "codex-local",
          modelId: "gpt-5.6-sol",
          providerManaged: false,
          options: [],
        },
      },
    });
    mounted.push({ component, target });
    const control = target.querySelector<HTMLElement>(".model-control");
    const trigger = target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]");
    const chevron = trigger?.querySelector<SVGElement>(".model-chevron");

    trigger?.click();
    await tick();

    expect(control?.style.width).toBe("");
    expect(chevron?.classList.contains("open")).toBe(true);

    trigger?.click();
    await tick();

    expect(control?.style.width).toBe("");
    expect(chevron?.classList.contains("open")).toBe(false);
  });

  it("prefers opening the main picker below and flips above when needed", async () => {
    const target = document.createElement("div");
    document.body.append(target);
    const component = mount(ChatModelControls, {
      target,
      props: {
        value: {
          providerInstanceId: "codex-local",
          modelId: "gpt-5.6-sol",
          providerManaged: false,
          options: [],
        },
      },
    });
    mounted.push({ component, target });
    const trigger = target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]");

    trigger?.click();
    await tick();
    const picker = document.querySelector<HTMLElement>(".model-popover");
    expect(picker?.parentElement).toBe(document.body);
    if (!trigger || !picker) throw new Error("Main model picker did not render");
    const triggerRect = vi.spyOn(trigger, "getBoundingClientRect").mockReturnValue(domRect(400, 200, 120, 32));
    vi.spyOn(picker, "getBoundingClientRect").mockReturnValue(domRect(0, 0, 296, 180));
    vi.stubGlobal("innerWidth", 1024);
    vi.stubGlobal("innerHeight", 768);

    window.dispatchEvent(new Event("resize"));
    await tick();

    expect(picker.style.left).toBe("400px");
    expect(picker.style.top).toBe("239px");
    expect(picker.classList.contains("below")).toBe(true);

    triggerRect.mockReturnValue(domRect(400, 650, 220, 32));
    window.dispatchEvent(new Event("resize"));
    await tick();

    expect(picker.style.left).toBe("400px");
    expect(picker.style.top).toBe("463px");
    expect(picker.classList.contains("below")).toBe(false);
  });

  it("ports advanced flyouts above scroll containers and prefers the right side", async () => {
    const target = document.createElement("div");
    document.body.append(target);
    const component = mount(ChatModelControls, {
      target,
      props: {
        value: {
          providerInstanceId: "codex-local",
          modelId: "gpt-5.6-sol",
          providerManaged: false,
          options: [],
        },
      },
    });
    mounted.push({ component, target });

    target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]")?.click();
    await tick();
    document.querySelector<HTMLButtonElement>(".model-popover .advanced-toggle")?.click();
    await tick();
    const modelRow = [...document.querySelectorAll<HTMLButtonElement>(".model-popover .advanced-list button")]
      .find((button) => button.textContent?.startsWith("Model"));
    modelRow?.dispatchEvent(new MouseEvent("pointerenter"));
    await tick();

    const picker = document.querySelector<HTMLElement>(".model-popover");
    const flyout = document.querySelector<HTMLElement>(".model-flyout");
    expect(picker?.parentElement).toBe(document.body);
    expect(flyout?.parentElement).toBe(document.body);
    if (!picker || !modelRow || !flyout) throw new Error("Advanced model flyout did not render");
    vi.spyOn(picker, "getBoundingClientRect").mockReturnValue(domRect(400, 80, 300, 220));
    vi.spyOn(modelRow, "getBoundingClientRect").mockReturnValue(domRect(410, 100, 280, 32));
    vi.spyOn(flyout, "getBoundingClientRect").mockReturnValue(domRect(0, 0, 200, 180));
    vi.stubGlobal("innerWidth", 1024);
    vi.stubGlobal("innerHeight", 768);

    window.dispatchEvent(new Event("resize"));
    await tick();

    expect(flyout.style.left).toBe("696px");
    expect(flyout.style.top).toBe("96px");
  });

  it("keeps the open flyout while the pointer crosses other rows toward it", async () => {
    const target = document.createElement("div");
    document.body.append(target);
    const component = mount(ChatModelControls, {
      target,
      props: {
        value: {
          providerInstanceId: "codex-local",
          modelId: "gpt-5.6-sol",
          providerManaged: false,
          options: [],
        },
      },
    });
    mounted.push({ component, target });

    target.querySelector<HTMLButtonElement>("[data-chat-model-trigger]")?.click();
    await tick();
    document.querySelector<HTMLButtonElement>(".model-popover .advanced-toggle")?.click();
    await tick();
    const rows = [...document.querySelectorAll<HTMLButtonElement>(".model-popover .advanced-list button[aria-expanded]")];
    const modelRow = rows.find((button) => button.textContent?.startsWith("Model"));
    const otherRow = rows.find((button) => button !== modelRow);
    if (!modelRow || !otherRow) throw new Error("Advanced rows did not render");
    modelRow.dispatchEvent(new MouseEvent("pointerenter"));
    await tick();
    const flyout = document.querySelector<HTMLElement>(".model-flyout");
    if (!flyout) throw new Error("Advanced model flyout did not render");
    vi.spyOn(modelRow, "getBoundingClientRect").mockReturnValue(domRect(410, 100, 280, 32));
    vi.spyOn(flyout, "getBoundingClientRect").mockReturnValue(domRect(696, 96, 280, 240));

    otherRow.dispatchEvent(new MouseEvent("pointermove", { bubbles: true, clientX: 600, clientY: 140 }));
    await tick();
    expect(modelRow.getAttribute("aria-expanded")).toBe("true");

    otherRow.dispatchEvent(new MouseEvent("pointermove", { bubbles: true, clientX: 540, clientY: 140 }));
    await tick();
    expect(modelRow.getAttribute("aria-expanded")).toBe("false");
    expect(otherRow.getAttribute("aria-expanded")).toBe("true");
  });
});
