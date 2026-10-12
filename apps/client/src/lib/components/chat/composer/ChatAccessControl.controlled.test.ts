// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as chatApi from "$lib/api/chat";
import type { SafetyMode } from "$lib/chat/contracts";
import { getChat } from "$lib/stores/chat.svelte";
import { modelSettings, resetChatComposerTestStore } from "$lib/components/chat/ChatComposer.test-support";
import ChatAccessControl from "./ChatAccessControl.svelte";

describe("ChatAccessControl controlled mode", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    resetChatComposerTestStore();
    getChat().settings = modelSettings();
    vi.spyOn(chatApi, "hasChatFullAccessTrust").mockResolvedValue(false);
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    vi.restoreAllMocks();
  });

  it("emits a teammate approval policy without mutating the live composer", async () => {
    getChat().composer.safetyMode = "full_access";
    const changes: SafetyMode[] = [];
    ({ target, component } = setup({
      value: "ask_for_approval",
      onChange: (value) => changes.push(value),
    }));

    target.querySelector<HTMLButtonElement>("[data-chat-field='safety']")?.click();
    await tick();
    const approveForMe = [...document.body.querySelectorAll<HTMLButtonElement>("[role='listbox'] button")]
      .find((button) => button.textContent?.includes("Approve for me"));
    approveForMe?.click();
    await tick();

    expect(changes).toEqual(["approve_for_me"]);
    expect(getChat().composer.safetyMode).toBe("full_access");
  });

  it("uses the controlled provider and folder when broad access is trusted", async () => {
    const setTrust = vi.spyOn(chatApi, "setChatFullAccessTrust").mockResolvedValue(true);
    const changes: SafetyMode[] = [];
    ({ target, component } = setup({
      value: "ask_for_approval",
      onChange: (value) => changes.push(value),
    }));

    target.querySelector<HTMLButtonElement>("[data-chat-field='safety']")?.click();
    await tick();
    const fullAccess = [...document.body.querySelectorAll<HTMLButtonElement>("[role='listbox'] button")]
      .find((button) => button.textContent?.includes("Full access"));
    fullAccess?.click();
    await tick();
    const confirm = [...document.body.querySelectorAll<HTMLButtonElement>(".confirm-dialog button")]
      .find((button) => button.textContent?.includes("Allow Full access"));
    confirm?.click();
    await tick();

    expect(setTrust).toHaveBeenCalledWith("codex-local", "teammate-folder", true);
    expect(changes).toEqual(["full_access"]);
  });

  it("stores a teammate permission preference before a channel target exists", async () => {
    const changes: SafetyMode[] = [];
    ({ target, component } = setup({
      value: "ask_for_approval",
      workingFolderId: null,
      onChange: (value) => changes.push(value),
    }));

    target.querySelector<HTMLButtonElement>("[data-chat-field='safety']")?.click();
    await tick();
    [...document.body.querySelectorAll<HTMLButtonElement>("[role='listbox'] button")]
      .find((button) => button.textContent?.includes("Full access"))
      ?.click();
    await tick();
    [...document.body.querySelectorAll<HTMLButtonElement>(".confirm-dialog button")]
      .find((button) => button.textContent?.includes("Allow Full access"))
      ?.click();
    await tick();

    expect(changes).toEqual(["full_access"]);
  });

  it("prevents editing an archived teammate", async () => {
    ({ target, component } = setup({
      value: "ask_for_approval",
      disabled: true,
    }));

    const trigger = target.querySelector<HTMLButtonElement>("[data-chat-field='safety']");
    expect(trigger?.disabled).toBe(true);
    trigger?.click();
    await tick();
    expect(document.body.querySelector("[role='listbox'][aria-label='Safety']")).toBeNull();
  });

  it("preserves the historical Full access visual state", () => {
    ({ target, component } = setup({ value: "full_access" }));

    expect(target.querySelector(".access-control.controlled.full-access")).not.toBeNull();
    expect(target.querySelector(".control-trigger")?.textContent).toContain("Full access");
  });
});

function setup(props: {
  value: SafetyMode;
  disabled?: boolean;
  workingFolderId?: string | null;
  onChange?: (value: SafetyMode) => void;
}): { target: HTMLDivElement; component: ReturnType<typeof mount> } {
  const target = document.createElement("div");
  document.body.append(target);
  const component = mount(ChatAccessControl, {
    target,
    props: {
      ...props,
      providerInstanceId: "codex-local",
      workingFolderId: props.workingFolderId === undefined
        ? "teammate-folder"
        : props.workingFolderId,
    },
  });
  return { target, component };
}
