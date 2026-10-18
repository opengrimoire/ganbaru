import { describe, expect, it, vi } from "vitest";
import { withProviderStartTrust, withWorkingFolderTrust } from "./working-folder-trust";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("$lib/api/db", () => ({ ensureDbUrl: async () => "sqlite:vault" }));

const required = { code: "trust_decision_required", message: "Decide", retryable: true };

describe("user Git actions that need a trust decision", () => {
  it("runs once without asking when no decision is needed", async () => {
    const action = vi.fn().mockResolvedValue("status");
    const decide = vi.fn();
    expect(await withWorkingFolderTrust("folder", action, decide)).toBe("status");
    expect(action).toHaveBeenCalledOnce();
    expect(decide).not.toHaveBeenCalled();
  });

  it("asks once for the folder and retries after a decision", async () => {
    const action = vi.fn().mockRejectedValueOnce(required).mockResolvedValueOnce("status");
    const decide = vi.fn().mockResolvedValue({ trust: "untrusted" });
    expect(await withWorkingFolderTrust("folder", action, decide)).toBe("status");
    expect(decide).toHaveBeenCalledExactlyOnceWith("folder");
    expect(action).toHaveBeenCalledTimes(2);
  });

  it("surfaces the original failure when the dialog is cancelled", async () => {
    const action = vi.fn().mockRejectedValue(required);
    const decide = vi.fn().mockResolvedValue(null);
    await expect(withWorkingFolderTrust("folder", action, decide)).rejects.toBe(required);
    expect(action).toHaveBeenCalledOnce();
  });

  it("does not ask again when the retry still needs a decision", async () => {
    const action = vi.fn().mockRejectedValue(required);
    const decide = vi.fn().mockResolvedValue({ trust: "trusted" });
    await expect(withWorkingFolderTrust("folder", action, decide)).rejects.toBe(required);
    expect(decide).toHaveBeenCalledOnce();
    expect(action).toHaveBeenCalledTimes(2);
  });

  it("passes other failures through without asking", async () => {
    const failure = { code: "conflict", message: "Busy", retryable: true };
    const decide = vi.fn();
    await expect(withWorkingFolderTrust("folder", vi.fn().mockRejectedValue(failure), decide)).rejects.toBe(failure);
    expect(decide).not.toHaveBeenCalled();
  });
});

describe("provider starts that need a trust decision", () => {
  const providerRequired = { ...required, details: { workingFolderId: "folder" } };

  it("asks for the folder named by the failure and retries", async () => {
    const action = vi.fn().mockRejectedValueOnce(providerRequired).mockResolvedValueOnce("sent");
    const decide = vi.fn().mockResolvedValue({ trust: "trusted" });
    expect(await withProviderStartTrust(action, decide)).toBe("sent");
    expect(decide).toHaveBeenCalledExactlyOnceWith("folder");
    expect(action).toHaveBeenCalledTimes(2);
  });

  it("surfaces the failure when it names no folder", async () => {
    const decide = vi.fn();
    await expect(withProviderStartTrust(vi.fn().mockRejectedValue(required), decide)).rejects.toBe(required);
    expect(decide).not.toHaveBeenCalled();
  });

  it("surfaces the original failure when the dialog is cancelled", async () => {
    const action = vi.fn().mockRejectedValue(providerRequired);
    const decide = vi.fn().mockResolvedValue(null);
    await expect(withProviderStartTrust(action, decide)).rejects.toBe(providerRequired);
    expect(action).toHaveBeenCalledOnce();
  });
});
