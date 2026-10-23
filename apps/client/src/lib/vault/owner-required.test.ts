import { afterEach, describe, expect, it, vi } from "vitest";
import {
  isOwnerRequiredError,
  onVaultOwnershipPromptRequest,
  ownerRequiredError,
  requestVaultOwnershipPrompt,
  withOwnerRequiredPrompt,
} from "./owner-required";

describe("owner-required errors", () => {
  const unsubscribers: Array<() => void> = [];

  afterEach(() => {
    for (const unsubscribe of unsubscribers.splice(0)) unsubscribe();
  });

  function listen(): ReturnType<typeof vi.fn> {
    const listener = vi.fn();
    unsubscribers.push(onVaultOwnershipPromptRequest(listener));
    return listener;
  }

  it("recognizes the refusal as a native string, an Error, and a structured command error", () => {
    expect(isOwnerRequiredError("owner_required: only the vault owner can do this")).toBe(true);
    expect(isOwnerRequiredError(new Error("owner_required: refused"))).toBe(true);
    expect(isOwnerRequiredError({ code: "permission", message: "owner_required: refused", retryable: true })).toBe(true);
    expect(isOwnerRequiredError(ownerRequiredError("projects are read-only here"))).toBe(true);
  });

  it("does not treat other failures or a prefix inside the text as the refusal", () => {
    expect(isOwnerRequiredError("This vault is read-only on this device")).toBe(false);
    expect(isOwnerRequiredError(new Error("failed: owner_required: nested"))).toBe(false);
    expect(isOwnerRequiredError({ code: "permission", message: 42 })).toBe(false);
    expect(isOwnerRequiredError(null)).toBe(false);
    expect(isOwnerRequiredError(undefined)).toBe(false);
  });

  it("asks for the prompt and rethrows when a command is refused", async () => {
    const listener = listen();
    const refusal = new Error("owner_required: refused");
    await expect(withOwnerRequiredPrompt(() => Promise.reject(refusal))).rejects.toBe(refusal);
    expect(listener).toHaveBeenCalledOnce();
  });

  it("leaves other failures and successes without a prompt", async () => {
    const listener = listen();
    await expect(withOwnerRequiredPrompt(() => Promise.reject(new Error("disk full")))).rejects.toThrow("disk full");
    await expect(withOwnerRequiredPrompt(() => Promise.resolve(7))).resolves.toBe(7);
    expect(listener).not.toHaveBeenCalled();
  });

  it("stops notifying an unsubscribed listener and keeps notifying others after one throws", () => {
    const failing = vi.fn(() => { throw new Error("broken"); });
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    unsubscribers.push(onVaultOwnershipPromptRequest(failing));
    const kept = listen();
    const removed = vi.fn();
    onVaultOwnershipPromptRequest(removed)();
    requestVaultOwnershipPrompt();
    expect(failing).toHaveBeenCalledOnce();
    expect(kept).toHaveBeenCalledOnce();
    expect(removed).not.toHaveBeenCalled();
    warn.mockRestore();
  });
});
