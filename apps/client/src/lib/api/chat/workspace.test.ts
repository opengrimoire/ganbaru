import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {},
  invoke: (...args: unknown[]) => invokeMock(...(args as [string, unknown])),
}));

vi.mock("$lib/api/db", () => ({
  ensureDbUrl: async () => "sqlite:test",
}));

beforeEach(() => {
  invokeMock.mockReset();
});

const origin = "http://127.0.0.1:5173";

describe("parsePreviewApproval", () => {
  it("accepts agent approvals and page requests", async () => {
    const { parsePreviewApproval } = await import("./workspace");
    expect(parsePreviewApproval(null)).toBeNull();
    expect(parsePreviewApproval({ kind: "network" })).toEqual({ kind: "network" });
    expect(parsePreviewApproval({ kind: "navigation", origin })).toEqual({ kind: "navigation", origin });
    expect(parsePreviewApproval({ kind: "permission", requestId: 3, origin, permission: "cameraAndMicrophone" }))
      .toEqual({ kind: "permission", requestId: 3, origin, permission: "cameraAndMicrophone" });
    expect(parsePreviewApproval({
      kind: "dialog",
      requestId: 4,
      origin,
      dialogKind: "beforeUnload",
      message: "",
      defaultText: "",
    })).toEqual({ kind: "dialog", requestId: 4, origin, dialogKind: "beforeUnload", message: "", defaultText: "" });
  });

  it("rejects page requests the panel could not name or answer", async () => {
    const { parsePreviewApproval } = await import("./workspace");
    const invalid: unknown[] = [
      "network",
      { kind: "navigation" },
      { kind: "permission", requestId: 3, origin, permission: "everything" },
      { kind: "permission", requestId: 0, origin, permission: "camera" },
      { kind: "permission", requestId: 1.5, origin, permission: "camera" },
      { kind: "permission", requestId: "3", origin, permission: "camera" },
      { kind: "permission", requestId: 3, permission: "camera" },
      { kind: "dialog", requestId: 4, origin, dialogKind: "print", message: "", defaultText: "" },
      { kind: "dialog", requestId: 4, origin, dialogKind: "prompt", message: "Name?" },
      { kind: "download", requestId: 5, origin },
    ];
    for (const value of invalid) {
      expect(() => parsePreviewApproval(value)).toThrow("Invalid browser approval");
    }
  });
});

describe("resolveBrowserApproval", () => {
  it("answers a page request by its ID with the prompt text", async () => {
    const { resolveBrowserApproval } = await import("./workspace");
    invokeMock.mockResolvedValue(null);
    await expect(resolveBrowserApproval("thread:test", true, { requestId: 7, text: "Ada" })).resolves.toBeNull();
    expect(invokeMock).toHaveBeenCalledWith("chat_browser_resolve_approval", {
      dbUrl: "sqlite:test",
      threadId: "thread:test",
      allow: true,
      requestId: 7,
      text: "Ada",
    });
  });

  it("answers the agent approval when no page request is named", async () => {
    const { resolveBrowserApproval } = await import("./workspace");
    invokeMock.mockResolvedValue(null);
    await resolveBrowserApproval("thread:test", false);
    expect(invokeMock).toHaveBeenCalledWith("chat_browser_resolve_approval", {
      dbUrl: "sqlite:test",
      threadId: "thread:test",
      allow: false,
      requestId: null,
      text: null,
    });
  });
});

describe("terminal layout commands", () => {
  it("name the channel and working folder so Rust authorizes the channel in it", async () => {
    const { readChatTerminalLayout, saveChatTerminalPanelLayout } = await import("./workspace");
    const layout = { channelId: "channel-1", workingFolderId: "folder-1", schemaVersion: 1, groups: [], updatedAt: null };
    invokeMock.mockResolvedValue(layout);
    await readChatTerminalLayout("channel-1", "folder-1");
    expect(invokeMock).toHaveBeenLastCalledWith("chat_read_terminal_layout", {
      dbUrl: "sqlite:test",
      channelId: "channel-1",
      workingFolderId: "folder-1",
    });
    const panel = {
      placement: "bottom" as const,
      terminalNames: ["Shell"],
      selectedIndex: 0,
      splitDirection: "horizontal" as const,
      splitSizes: [],
    };
    await saveChatTerminalPanelLayout("channel-1", "folder-1", panel);
    expect(invokeMock).toHaveBeenLastCalledWith("chat_save_terminal_panel_layout", {
      dbUrl: "sqlite:test",
      channelId: "channel-1",
      workingFolderId: "folder-1",
      panel,
    });
  });

  it("start terminals and discover preview servers in the panel's channel and folder", async () => {
    const { createChatTerminal, discoverPreviewServers } = await import("./workspace");
    invokeMock.mockRejectedValueOnce(new Error("stop after the call"));
    const request = {
      terminalId: "0f8fad5b-d9cb-469f-a165-70867728950e",
      channelId: "channel-1",
      workingFolderId: "folder-2",
      columns: 80,
      rows: 24,
    };
    await expect(createChatTerminal(request)).rejects.toThrow("stop after the call");
    expect(invokeMock).toHaveBeenLastCalledWith("chat_terminal_create", { dbUrl: "sqlite:test", request });

    invokeMock.mockResolvedValueOnce([]);
    await expect(discoverPreviewServers("channel-1", "folder-2")).resolves.toEqual([]);
    expect(invokeMock).toHaveBeenLastCalledWith("chat_browser_discover_servers", {
      dbUrl: "sqlite:test",
      channelId: "channel-1",
      workingFolderId: "folder-2",
    });
  });
});
