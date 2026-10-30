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
        "selected folder is not empty and is not a Ganbaru folder",
        "default",
      ),
    ).toBe(
      "The default folder contains other files. Move them or choose another folder.",
    );
  });

  it("formats import and marker errors as user-facing folder guidance", async () => {
    const { formatVaultError } = await loadModule();

    expect(formatVaultError("selected folder is not a Ganbaru folder", "import")).toBe(
      "Not a Ganbaru folder. Choose your previous app folder.",
    );
    expect(
      formatVaultError(
        "read Ganbaru folder marker: No such file or directory (os error 2)",
        "import",
      ),
    ).toBe(
      "Can't identify this folder. Choose the main Ganbaru folder.",
    );
    expect(formatVaultError("parse Ganbaru folder marker: expected value", "import")).toBe(
      "This folder's information is damaged. Restore a backup or choose another.",
    );
  });

  it("formats permission and database errors without raw backend text", async () => {
    const { formatVaultError } = await loadModule();

    expect(formatVaultError("read Ganbaru folder: Permission denied")).toBe(
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

    expect(formatVaultError("canonicalize Ganbaru folder path: No such file", "startup")).toBe(
      "Folder not found. Choose another or import an existing one.",
    );
  });

  it("explains a vault another Ganbaru app holds before generic folder guidance", async () => {
    const { formatVaultError } = await loadModule();
    const expected =
      "This folder is open in another Ganbaru app. Close that app or choose another folder.";

    expect(formatVaultError("this vault is open in another Ganbaru app", "change")).toBe(expected);
    expect(
      formatVaultError(new Error("this vault is open in another Ganbaru app"), "startup"),
    ).toBe(expected);
  });

  it("reads the vault lock status and rejects unknown values", async () => {
    const { getVaultLockStatus, parseVaultLockStatus } = await loadModule();

    for (const status of ["notHeld", "exclusive", "unsupported"]) {
      expect(parseVaultLockStatus(status)).toBe(status);
    }
    for (const value of ["Exclusive", "", null, undefined, 1, { status: "exclusive" }]) {
      expect(() => parseVaultLockStatus(value)).toThrow("vault lock status response is invalid");
    }

    invokeMock.mockResolvedValue("unsupported");
    await expect(getVaultLockStatus()).resolves.toBe("unsupported");
    expect(invokeMock).toHaveBeenCalledWith("vault_lock_status");
  });

  it("accepts complete vault info responses", async () => {
    invokeMock.mockResolvedValue({
      path: "/home/user/Documents/Ganbaru",
      databasePath: "/home/user/Documents/Ganbaru/ganbaru.sqlite",
      vaultId: "vault-1",
      displayName: "Ganbaru",
    });
    const { getActiveVaultInfo, getCachedActiveVaultInfo } = await loadModule();

    expect(getCachedActiveVaultInfo()).toBeUndefined();

    await expect(getActiveVaultInfo()).resolves.toEqual({
      path: "/home/user/Documents/Ganbaru",
      databasePath: "/home/user/Documents/Ganbaru/ganbaru.sqlite",
      vaultId: "vault-1",
      displayName: "Ganbaru",
    });
    expect(getCachedActiveVaultInfo()).toEqual({
      path: "/home/user/Documents/Ganbaru",
      databasePath: "/home/user/Documents/Ganbaru/ganbaru.sqlite",
      vaultId: "vault-1",
      displayName: "Ganbaru",
    });
  });

  it("rejects incomplete vault info responses", async () => {
    invokeMock.mockResolvedValue({
      path: "/home/user/Documents/Ganbaru",
      databasePath: "/home/user/Documents/Ganbaru/ganbaru.sqlite",
      vaultId: "vault-1",
    });
    const { getActiveVaultInfo } = await loadModule();

    await expect(getActiveVaultInfo()).rejects.toThrow("vault info response is incomplete");
  });

  it("reads the default vault location", async () => {
    invokeMock.mockResolvedValue({
      path: "/home/user/Documents/Ganbaru",
      parentPath: "/home/user/Documents",
      folderName: "Ganbaru",
      developmentBuild: false,
    });
    const { getDefaultVaultLocation } = await loadModule();

    await expect(getDefaultVaultLocation()).resolves.toEqual({
      path: "/home/user/Documents/Ganbaru",
      parentPath: "/home/user/Documents",
      folderName: "Ganbaru",
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
      linkedReplica: false,
      replicaWritableDomains: [],
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
      linkedReplica: false,
      replicaWritableDomains: [],
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
      linkedReplica: false,
      replicaWritableDomains: [],
      transferPhase: { kind: "stable" },
    });
    const { getVaultOwnershipStatus } = await loadModule();

    await expect(getVaultOwnershipStatus()).rejects.toThrow(
      "vault ownership response has an inconsistent role",
    );
  });

  it("accepts linked replicas and their writable domains only on read-only replicas", async () => {
    const replica = {
      vaultId: "vault-1",
      deviceId: "phone",
      ownerDeviceId: "desktop",
      generation: 4,
      role: "read-only",
      canWrite: false,
      linkedReplica: true,
      replicaWritableDomains: ["quick_notes"],
      transferPhase: { kind: "stable" },
    };
    invokeMock.mockResolvedValueOnce(replica);
    const { getVaultOwnershipStatus } = await loadModule();
    await expect(getVaultOwnershipStatus()).resolves.toMatchObject({
      linkedReplica: true,
      replicaWritableDomains: ["quick_notes"],
      canWrite: false,
    });

    invokeMock.mockResolvedValueOnce({ ...replica, role: "owner", canWrite: true });
    await expect(getVaultOwnershipStatus()).rejects.toThrow("inconsistent linked replica");

    invokeMock.mockResolvedValueOnce({ ...replica, linkedReplica: false });
    await expect(getVaultOwnershipStatus()).rejects.toThrow("inconsistent replica writes");

    for (const replicaWritableDomains of [undefined, "quick_notes", ["quick_notes", 3], [" "]]) {
      invokeMock.mockResolvedValueOnce({ ...replica, replicaWritableDomains });
      await expect(getVaultOwnershipStatus()).rejects.toThrow("vault ownership response is incomplete");
    }

    const { linkedReplica: _omitted, ...withoutFlag } = replica;
    invokeMock.mockResolvedValueOnce(withoutFlag);
    await expect(getVaultOwnershipStatus()).rejects.toThrow("vault ownership response is incomplete");
  });

  it("admits sync domain writes for owners and for linked replicas the domain lets write", async () => {
    const { QUICK_NOTES_SYNC_DOMAIN, canWriteSyncDomain } = await loadModule();
    const base = {
      vaultId: "vault-1",
      deviceId: "phone",
      ownerDeviceId: "desktop",
      generation: 4,
      transferPhase: { kind: "stable" as const },
    };
    const owner = { ...base, role: "owner" as const, canWrite: true, linkedReplica: false, replicaWritableDomains: [] };
    const replica = {
      ...base,
      role: "read-only" as const,
      canWrite: false,
      linkedReplica: true,
      replicaWritableDomains: [QUICK_NOTES_SYNC_DOMAIN],
    };
    const unlinked = { ...replica, linkedReplica: false, replicaWritableDomains: [] };

    expect(canWriteSyncDomain(owner, "projects")).toBe(true);
    expect(canWriteSyncDomain(replica, QUICK_NOTES_SYNC_DOMAIN)).toBe(true);
    expect(canWriteSyncDomain(replica, "projects")).toBe(false);
    expect(canWriteSyncDomain(unlinked, QUICK_NOTES_SYNC_DOMAIN)).toBe(false);
  });
});

describe("database rollbacks", () => {
  const rollback = {
    id: "20261018T070005000Z",
    createdAt: "2026-10-18T07:00:05.000Z",
    sizeBytes: 4096,
    origin: "reset",
    flagged: false,
    dismissed: false,
  };

  it("lists rollbacks in the order Rust returned", async () => {
    const { listDatabaseRollbacks } = await loadModule();
    const older = { ...rollback, id: "20261017T070005000Z", createdAt: "2026-10-17T07:00:05.000Z" };
    invokeMock.mockResolvedValueOnce([rollback, older]);
    await expect(listDatabaseRollbacks()).resolves.toEqual([rollback, older]);
    expect(invokeMock).toHaveBeenCalledWith("vault_database_rollbacks");
  });

  it("rejects malformed rollback entries", async () => {
    const { parseDatabaseRollbacks } = await loadModule();
    expect(() => parseDatabaseRollbacks(null)).toThrow("not a list");
    expect(() => parseDatabaseRollbacks(["x"])).toThrow("not an object");
    expect(() => parseDatabaseRollbacks([{ ...rollback, id: "" }])).toThrow("incomplete");
    expect(() => parseDatabaseRollbacks([{ ...rollback, createdAt: "yesterday" }])).toThrow("incomplete");
    expect(() => parseDatabaseRollbacks([{ ...rollback, sizeBytes: -1 }])).toThrow("incomplete");
    expect(() => parseDatabaseRollbacks([{ ...rollback, sizeBytes: 1.5 }])).toThrow("incomplete");
    expect(() => parseDatabaseRollbacks([{ ...rollback, origin: "handoff" }])).toThrow("incomplete");
    expect(() => parseDatabaseRollbacks([{ ...rollback, flagged: "yes" }])).toThrow("incomplete");
    const { dismissed: _omitted, ...withoutDismissed } = rollback;
    expect(() => parseDatabaseRollbacks([withoutDismissed])).toThrow("incomplete");
  });

  it("reports only undismissed flagged replacement copies", async () => {
    const { parseDatabaseRollbacks, pendingFlaggedRollbacks } = await loadModule();
    const flagged = { ...rollback, id: "20261018T080005000Z", origin: "replacement", flagged: true };
    const dismissed = { ...flagged, id: "20261018T090005000Z", dismissed: true };
    const plain = { ...rollback, origin: "replacement" };
    const parsed = parseDatabaseRollbacks([dismissed, flagged, plain, rollback]);
    expect(pendingFlaggedRollbacks(parsed).map((entry) => entry.id)).toEqual([flagged.id]);
  });

  it("dismisses a flagged rollback by its ID only", async () => {
    const { dismissDatabaseRollback } = await loadModule();
    invokeMock.mockResolvedValueOnce(undefined);
    await dismissDatabaseRollback(rollback.id);
    expect(invokeMock).toHaveBeenCalledWith("vault_dismiss_database_rollback", { id: rollback.id });
  });

  it("restores a rollback by its ID only", async () => {
    const { restoreDatabaseRollback } = await loadModule();
    invokeMock.mockResolvedValueOnce(undefined);
    await restoreDatabaseRollback(rollback.id);
    expect(invokeMock).toHaveBeenCalledWith("vault_restore_database_rollback", { id: rollback.id });
  });
});

describe("replaced vault backups", () => {
  const backup = {
    id: "replaced-1761700000000.ganbaru-backup",
    displayName: "Ganbaru",
    replacedAtMs: 1_761_700_000_000,
    sizeBytes: 8192,
  };

  it("lists backups in the order Rust returned, with or without a vault name", async () => {
    const { listReplacedVaultBackups } = await loadModule();
    const unnamed = { ...backup, id: "replaced-1761600000000.ganbaru-backup", displayName: null };
    invokeMock.mockResolvedValueOnce([backup, unnamed]);
    await expect(listReplacedVaultBackups()).resolves.toEqual([backup, unnamed]);
    expect(invokeMock).toHaveBeenCalledWith("vault_replaced_backups");
  });

  it("rejects malformed backup entries", async () => {
    const { parseReplacedVaultBackups } = await loadModule();
    expect(() => parseReplacedVaultBackups({})).toThrow("not a list");
    expect(() => parseReplacedVaultBackups([7])).toThrow("not an object");
    expect(() => parseReplacedVaultBackups([{ ...backup, id: " " }])).toThrow("incomplete");
    expect(() => parseReplacedVaultBackups([{ ...backup, displayName: "" }])).toThrow("incomplete");
    expect(() => parseReplacedVaultBackups([{ ...backup, displayName: 3 }])).toThrow("incomplete");
    expect(() => parseReplacedVaultBackups([{ ...backup, replacedAtMs: -1 }])).toThrow("incomplete");
    expect(() => parseReplacedVaultBackups([{ ...backup, sizeBytes: 0.5 }])).toThrow("incomplete");
  });

  it("exports a backup by its ID and validates the saved file", async () => {
    const { exportReplacedVaultBackup } = await loadModule();
    invokeMock.mockResolvedValueOnce({ fileName: "replaced.ganbaru-backup", destination: "downloads" });
    await expect(exportReplacedVaultBackup(backup.id)).resolves.toEqual({
      fileName: "replaced.ganbaru-backup",
      destination: "downloads",
    });
    expect(invokeMock).toHaveBeenCalledWith("vault_export_replaced_backup", { id: backup.id });
    invokeMock.mockResolvedValueOnce({ fileName: "x", destination: "elsewhere" });
    await expect(exportReplacedVaultBackup(backup.id)).rejects.toThrow("incomplete");
  });

  it("deletes a backup by its ID only", async () => {
    const { deleteReplacedVaultBackup } = await loadModule();
    invokeMock.mockResolvedValueOnce(undefined);
    await deleteReplacedVaultBackup(backup.id);
    expect(invokeMock).toHaveBeenCalledWith("vault_delete_replaced_backup", { id: backup.id });
  });
});

describe("encrypted backups", () => {
  const vault = {
    path: "/home/user/Restored",
    databasePath: "/home/user/Restored/ganbaru.sqlite",
    vaultId: "vault-restored",
    displayName: "Restored",
  };

  it("parses every save outcome and rejects unknown refusals", async () => {
    const { parseBackupSaveOutcome } = await loadModule();
    expect(parseBackupSaveOutcome({ kind: "cancelled" })).toEqual({ kind: "cancelled" });
    expect(parseBackupSaveOutcome({ kind: "saved", fileName: "a.ganbaru-backup" })).toEqual({
      kind: "saved",
      fileName: "a.ganbaru-backup",
    });
    expect(parseBackupSaveOutcome({ kind: "refused", reason: "noRecoveryKit" })).toEqual({
      kind: "refused",
      reason: "noRecoveryKit",
    });
    expect(() => parseBackupSaveOutcome({ kind: "refused", reason: "other" })).toThrow("unknown");
    expect(() => parseBackupSaveOutcome({ kind: "saved", fileName: " " })).toThrow("incomplete");
    expect(() => parseBackupSaveOutcome({ kind: "written" })).toThrow("unknown");
    expect(() => parseBackupSaveOutcome(null)).toThrow("not an object");
  });

  it("requires a token and a kit flag on a ready inspection", async () => {
    const { parseBackupInspection } = await loadModule();
    expect(parseBackupInspection({ kind: "ready", token: "t1", needsKit: true })).toEqual({
      kind: "ready",
      token: "t1",
      needsKit: true,
    });
    expect(() => parseBackupInspection({ kind: "ready", token: "t1" })).toThrow("incomplete");
    expect(() => parseBackupInspection({ kind: "ready", token: "", needsKit: false })).toThrow(
      "incomplete",
    );
    expect(parseBackupInspection({ kind: "refused", reason: "notBackup" })).toEqual({
      kind: "refused",
      reason: "notBackup",
    });
  });

  it("validates the restored vault and its placement", async () => {
    const { parseBackupRestoreOutcome } = await loadModule();
    expect(
      parseBackupRestoreOutcome({ kind: "restored", vault, placement: "independentCopy" }),
    ).toEqual({ kind: "restored", vault, placement: "independentCopy" });
    expect(() =>
      parseBackupRestoreOutcome({ kind: "restored", vault, placement: "merged" }),
    ).toThrow("placement");
    expect(() =>
      parseBackupRestoreOutcome({ kind: "restored", vault: { ...vault, vaultId: "" }, placement: "inPlace" }),
    ).toThrow("incomplete");
  });

  it("sends the inspected token and kit, and caches the restored vault as active", async () => {
    const { getCachedActiveVaultInfo, restoreInspectedBackup } = await loadModule();
    invokeMock.mockResolvedValueOnce({ kind: "restored", vault, placement: "inPlace" });
    await expect(restoreInspectedBackup("t1", "KIT")).resolves.toEqual({
      kind: "restored",
      vault,
      placement: "inPlace",
    });
    expect(invokeMock).toHaveBeenCalledWith("vault_restore_backup", { token: "t1", kit: "KIT" });
    expect(getCachedActiveVaultInfo()).toEqual(vault);
  });

  it("leaves the cached vault alone after a refused restore", async () => {
    const { getCachedActiveVaultInfo, restoreInspectedBackup } = await loadModule();
    invokeMock.mockResolvedValueOnce({ kind: "refused", reason: "wrongKit" });
    await expect(restoreInspectedBackup("t1", null)).resolves.toEqual({
      kind: "refused",
      reason: "wrongKit",
    });
    expect(invokeMock).toHaveBeenCalledWith("vault_restore_backup", { token: "t1", kit: null });
    expect(getCachedActiveVaultInfo()).toBeUndefined();
  });
});
