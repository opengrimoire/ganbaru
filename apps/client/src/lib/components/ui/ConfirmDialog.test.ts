// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import ConfirmDialog from "./ConfirmDialog.svelte";

describe("ConfirmDialog", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
  });

  it("adds the standard keyboard hints to custom button labels", () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ConfirmDialog, {
      target,
      props: {
        message: "Delete this item?",
        confirmLabel: "Delete",
        cancelLabel: "Keep",
        onConfirm: () => undefined,
        onCancel: () => undefined,
      },
    });

    expect(target.textContent).toContain("Keep (Esc)");
    expect(target.textContent).toContain("Delete (Enter)");
  });

  it("adds the standard keyboard hints to the default labels", () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ConfirmDialog, {
      target,
      props: {
        message: "Continue?",
        onConfirm: () => undefined,
        onCancel: () => undefined,
      },
    });

    expect(target.textContent).toContain("No (Esc)");
    expect(target.textContent).toContain("Yes (Enter)");
  });

  it("offers an alternate action between cancel and confirm that keys never trigger", async () => {
    target = document.createElement("div");
    document.body.append(target);
    const onConfirm = vi.fn();
    const onAlternate = vi.fn();
    component = mount(ConfirmDialog, {
      target,
      props: {
        message: "Save the theme?",
        confirmLabel: "Save as copy",
        alternateLabel: "Overwrite",
        cancelLabel: "Keep editing",
        onConfirm,
        onCancel: vi.fn(),
        onAlternate,
      },
    });
    await tick();

    const buttons = target.querySelectorAll<HTMLButtonElement>(".confirm-dialog button");
    expect([...buttons].map((button) => button.textContent?.trim())).toEqual([
      "Keep editing (Esc)",
      "Overwrite",
      "Save as copy (Enter)",
    ]);
    target.querySelector<HTMLElement>(".confirm-dialog")?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }),
    );
    expect(onConfirm).toHaveBeenCalledOnce();
    expect(onAlternate).not.toHaveBeenCalled();

    buttons.item(1).click();
    expect(onAlternate).toHaveBeenCalledOnce();
  });

  it("hides the alternate action without a handler", () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ConfirmDialog, {
      target,
      props: {
        message: "Continue?",
        alternateLabel: "Other",
        onConfirm: () => undefined,
        onCancel: () => undefined,
      },
    });

    expect(target.querySelectorAll(".confirm-dialog button")).toHaveLength(2);
  });

  it("separates dismissing the dialog from its explicit cancel action", async () => {
    target = document.createElement("div");
    document.body.append(target);
    const onCancel = vi.fn();
    const onDismiss = vi.fn();
    component = mount(ConfirmDialog, {
      target,
      props: {
        message: "Resume the session?",
        onConfirm: vi.fn(),
        onCancel,
        onDismiss,
      },
    });
    await tick();

    target.querySelector<HTMLElement>(".confirm-dialog")?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    expect(onDismiss).toHaveBeenCalledOnce();
    expect(onCancel).not.toHaveBeenCalled();

    target.querySelectorAll<HTMLButtonElement>(".confirm-dialog button").item(0).click();
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it("keeps Enter and Escape authoritative while the safe cancel action has focus", async () => {
    target = document.createElement("div");
    document.body.append(target);
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    component = mount(ConfirmDialog, {
      target,
      props: {
        message: "Delete this item?",
        onConfirm,
        onCancel,
      },
    });
    await tick();

    const cancelButton = target.querySelector<HTMLButtonElement>(".confirm-dialog button");
    cancelButton?.focus();
    expect(document.activeElement).toBe(cancelButton);

    cancelButton?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }),
    );
    expect(onConfirm).toHaveBeenCalledOnce();
    expect(onCancel).not.toHaveBeenCalled();

    cancelButton?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    expect(onCancel).toHaveBeenCalledOnce();
  });
});
