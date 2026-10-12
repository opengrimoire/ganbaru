import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...(args as [string, unknown])),
}));

async function loadModule() {
  return await import("./state");
}

beforeEach(() => {
  vi.resetModules();
  invokeMock.mockReset();
});

describe("vault state api", () => {
  it("formats non-empty default folder errors with default-specific guidance", async () => {
    const { formatVaultError } = await loadModule();

    expect(
      formatVaultError(
        "selected folder is not empty and is not a Ganbaru AI folder",
        "default",
      ),
    ).toBe(
      "The default folder contains other files. Move them or choose another folder.",
    );
  });

  it("formats import and marker errors as user-facing folder guidance", async () => {
    const { formatVaultError } = await loadModule();

    expect(formatVaultError("selected folder is not a Ganbaru AI folder", "import")).toBe(
      "Not a Ganbaru AI folder. Choose your previous app folder.",
    );
    expect(
      formatVaultError(
        "read Ganbaru AI folder marker: No such file or directory (os error 2)",
        "import",
      ),
    ).toBe(
      "Can't identify this folder. Choose the main Ganbaru AI folder.",
    );
    expect(formatVaultError("parse Ganbaru AI folder marker: expected value", "import")).toBe(
      "This folder's information is damaged. Restore a backup or choose another.",
    );
  });

  it("formats permission and database errors without raw backend text", async () => {
    const { formatVaultError } = await loadModule();

    expect(formatVaultError("read Ganbaru AI folder: Permission denied")).toBe(
      "Can't access this folder. Check permissions or choose another.",
    );
    expect(
      formatVaultError("run database migrations: file is not a database", "startup"),
    ).toBe(
      "Can't open this folder's data. Restore a backup or choose another.",
    );
  });

  it("keeps unexpected backend details out of the setup message", async () => {
    const { formatVaultError } = await loadModule();

    expect(formatVaultError("unable to read /private/folder: internal error", "startup")).toBe(
      "Can't open this folder. Choose another or import an existing one.",
    );
  });

  it("gives a short recovery action for a missing folder", async () => {
    const { formatVaultError } = await loadModule();

    expect(formatVaultError("canonicalize Ganbaru AI folder path: No such file", "startup")).toBe(
      "Folder not found. Choose another or import an existing one.",
    );
  });

  it("normalizes malformed app state responses to an empty state", async () => {
    invokeMock.mockResolvedValue({ activeVaultPath: 123, recentVaultPaths: [null, ""] });
    const { readVaultAppState } = await loadModule();

    await expect(readVaultAppState()).resolves.toEqual({
      activeVaultPath: null,
      recentVaultPaths: [],
    });
  });

  it("accepts complete vault info responses", async () => {
    invokeMock.mockResolvedValue({
      path: "/home/user/Documents/Ganbaru AI",
      configPath: "/home/user/Documents/Ganbaru AI/config.json",
      databasePath: "/home/user/Documents/Ganbaru AI/ganbaru-ai.sqlite",
      vaultId: "vault-1",
      displayName: "Ganbaru AI",
    });
    const { getActiveVaultInfo, getCachedActiveVaultInfo } = await loadModule();

    expect(getCachedActiveVaultInfo()).toBeUndefined();

    await expect(getActiveVaultInfo()).resolves.toEqual({
      path: "/home/user/Documents/Ganbaru AI",
      configPath: "/home/user/Documents/Ganbaru AI/config.json",
      databasePath: "/home/user/Documents/Ganbaru AI/ganbaru-ai.sqlite",
      vaultId: "vault-1",
      displayName: "Ganbaru AI",
    });
    expect(getCachedActiveVaultInfo()).toEqual({
      path: "/home/user/Documents/Ganbaru AI",
      configPath: "/home/user/Documents/Ganbaru AI/config.json",
      databasePath: "/home/user/Documents/Ganbaru AI/ganbaru-ai.sqlite",
      vaultId: "vault-1",
      displayName: "Ganbaru AI",
    });
  });

  it("rejects incomplete vault info responses", async () => {
    invokeMock.mockResolvedValue({
      path: "/home/user/Documents/Ganbaru AI",
      configPath: "/home/user/Documents/Ganbaru AI/config.json",
      databasePath: "/home/user/Documents/Ganbaru AI/ganbaru-ai.sqlite",
      vaultId: "vault-1",
    });
    const { getActiveVaultInfo } = await loadModule();

    await expect(getActiveVaultInfo()).rejects.toThrow("vault info response is incomplete");
  });

  it("reads the default vault location", async () => {
    invokeMock.mockResolvedValue({
      path: "/home/user/Documents/Ganbaru AI",
      parentPath: "/home/user/Documents",
      folderName: "Ganbaru AI",
      developmentBuild: false,
    });
    const { getDefaultVaultLocation } = await loadModule();

    await expect(getDefaultVaultLocation()).resolves.toEqual({
      path: "/home/user/Documents/Ganbaru AI",
      parentPath: "/home/user/Documents",
      folderName: "Ganbaru AI",
      developmentBuild: false,
    });
  });

  it("validates the native vault ownership status", async () => {
    invokeMock.mockResolvedValue({
      vaultId: "vault-1",
      deviceId: "phone",
      ownerDeviceId: "desktop",
      generation: 4,
      role: "read-only",
      canWrite: false,
      replicatedWrites: false,
      transferPhase: { kind: "stable" },
    });
    const { getVaultOwnershipStatus } = await loadModule();

    await expect(getVaultOwnershipStatus()).resolves.toEqual({
      vaultId: "vault-1",
      deviceId: "phone",
      ownerDeviceId: "desktop",
      generation: 4,
      role: "read-only",
      canWrite: false,
      replicatedWrites: false,
      transferPhase: { kind: "stable" },
    });
  });

  it("rejects inconsistent writable ownership responses", async () => {
    invokeMock.mockResolvedValue({
      vaultId: "vault-1",
      deviceId: "phone",
      ownerDeviceId: "desktop",
      generation: 4,
      role: "read-only",
      canWrite: true,
      replicatedWrites: false,
      transferPhase: { kind: "stable" },
    });
    const { getVaultOwnershipStatus } = await loadModule();

    await expect(getVaultOwnershipStatus()).rejects.toThrow(
      "vault ownership response has an inconsistent role",
    );
  });
  it("accepts replicated writes only on read-only replicas", async () => {
    const replica = {
      vaultId: "vault-1",
      deviceId: "phone",
      ownerDeviceId: "desktop",
      generation: 4,
      role: "read-only",
      canWrite: false,
      replicatedWrites: true,
      transferPhase: { kind: "stable" },
    };
    invokeMock.mockResolvedValueOnce(replica);
    const { getVaultOwnershipStatus } = await loadModule();
    await expect(getVaultOwnershipStatus()).resolves.toMatchObject({ replicatedWrites: true, canWrite: false });

    invokeMock.mockResolvedValueOnce({ ...replica, role: "owner", canWrite: true });
    await expect(getVaultOwnershipStatus()).rejects.toThrow("inconsistent replicated writes");

    const { replicatedWrites: _omitted, ...withoutFlag } = replica;
    invokeMock.mockResolvedValueOnce(withoutFlag);
    await expect(getVaultOwnershipStatus()).rejects.toThrow("vault ownership response is incomplete");
  });
});
