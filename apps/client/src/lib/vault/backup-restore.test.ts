import { describe, expect, it, vi } from "vitest";
import {
  backupRefusalMessage,
  backupSaveStatus,
  refusalNeedsKit,
  runBackupRestore,
  type BackupRestoreSteps,
} from "./backup-restore";
import type { BackupRefusal, BackupRestoreOutcome, VaultInfo } from "./state";
import { translate } from "$lib/i18n/translator.svelte";

const vault: VaultInfo = {
  path: "/vaults/Restored",
  databasePath: "/vaults/Restored/ganbaru.sqlite",
  vaultId: "vault-restored",
  displayName: "Restored",
};

const restored: BackupRestoreOutcome = { kind: "restored", vault, placement: "independentCopy" };

function steps(
  overrides: Partial<BackupRestoreSteps> & { kits?: (string | null)[] },
): BackupRestoreSteps & { askKit: ReturnType<typeof vi.fn> } {
  const kits = [...(overrides.kits ?? [])];
  return {
    inspect: overrides.inspect ?? (async () => ({ kind: "ready", token: "t1", needsKit: false })),
    restore: overrides.restore ?? (async () => restored),
    askKit: vi.fn(async (_refusal: BackupRefusal | null) => kits.shift() ?? null),
  };
}

describe("runBackupRestore", () => {
  it("restores with this device's keys without asking for the kit", async () => {
    const restore = vi.fn(async () => restored);
    const flow = steps({ restore });
    await expect(runBackupRestore(flow)).resolves.toEqual(restored);
    expect(restore).toHaveBeenCalledWith("t1", null);
    expect(flow.askKit).not.toHaveBeenCalled();
  });

  it("asks for the kit first when the inspection says only the kit opens the backup", async () => {
    const restore = vi.fn(async () => restored);
    const flow = steps({
      inspect: async () => ({ kind: "ready", token: "t2", needsKit: true }),
      restore,
      kits: ["CODE"],
    });
    await expect(runBackupRestore(flow)).resolves.toEqual(restored);
    expect(flow.askKit).toHaveBeenCalledWith(null);
    expect(restore).toHaveBeenCalledWith("t2", "CODE");
  });

  it("asks again after a wrong or mistyped kit and passes the refusal to the prompt", async () => {
    const outcomes: BackupRestoreOutcome[] = [
      { kind: "refused", reason: "kitRequired" },
      { kind: "refused", reason: "invalidKit" },
      { kind: "refused", reason: "wrongKit" },
      restored,
    ];
    const restore = vi.fn(async () => outcomes.shift() ?? restored);
    const flow = steps({ restore, kits: ["A", "B", "C"] });
    await expect(runBackupRestore(flow)).resolves.toEqual(restored);
    expect(flow.askKit.mock.calls.map(([reason]) => reason)).toEqual([
      "kitRequired",
      "invalidKit",
      "wrongKit",
    ]);
    expect(restore.mock.calls.map(([, kit]) => kit)).toEqual([null, "A", "B", "C"]);
  });

  it("stops as cancelled when the user closes the kit prompt", async () => {
    const restore = vi.fn(async (): Promise<BackupRestoreOutcome> => ({ kind: "refused", reason: "wrongKit" }));
    const flow = steps({ restore, kits: [null] });
    await expect(runBackupRestore(flow)).resolves.toEqual({ kind: "cancelled" });
    expect(restore).toHaveBeenCalledTimes(1);
  });

  it("returns refusals a kit cannot fix without asking", async () => {
    const flow = steps({
      restore: async () => ({ kind: "refused", reason: "linkedAndroid" }),
    });
    await expect(runBackupRestore(flow)).resolves.toEqual({ kind: "refused", reason: "linkedAndroid" });
    expect(flow.askKit).not.toHaveBeenCalled();
  });

  it("returns a refused or cancelled inspection without restoring", async () => {
    const restore = vi.fn(async () => restored);
    await expect(
      runBackupRestore(steps({ inspect: async () => ({ kind: "refused", reason: "notBackup" }), restore })),
    ).resolves.toEqual({ kind: "refused", reason: "notBackup" });
    await expect(
      runBackupRestore(steps({ inspect: async () => ({ kind: "cancelled" }), restore })),
    ).resolves.toEqual({ kind: "cancelled" });
    expect(restore).not.toHaveBeenCalled();
  });
});

describe("backup messages", () => {
  it("marks only kit refusals as fixable by another kit", () => {
    expect(refusalNeedsKit("wrongKit")).toBe(true);
    expect(refusalNeedsKit("invalidKit")).toBe(true);
    expect(refusalNeedsKit("kitRequired")).toBe(true);
    expect(refusalNeedsKit("damaged")).toBe(false);
    expect(refusalNeedsKit("expired")).toBe(false);
  });

  it("gives every refusal its own message", () => {
    const reasons: BackupRefusal[] = [
      "noRecoveryKit",
      "notBackup",
      "unsupportedVersion",
      "damaged",
      "kitRequired",
      "wrongKit",
      "invalidKit",
      "linkedAndroid",
      "expired",
    ];
    const messages = reasons.map((reason) => backupRefusalMessage(reason, translate));
    expect(new Set(messages).size).toBe(reasons.length);
  });

  it("words a saved backup by destination and hides a cancelled save", () => {
    const saved = { kind: "saved", fileName: "a.ganbaru-backup" } as const;
    expect(backupSaveStatus(saved, "downloads", translate)).toEqual({
      kind: "success",
      message: "Saved a.ganbaru-backup to Downloads",
    });
    expect(backupSaveStatus(saved, "file", translate)).toEqual({
      kind: "success",
      message: "Saved a.ganbaru-backup",
    });
    expect(backupSaveStatus({ kind: "cancelled" }, "file", translate)).toBeNull();
    expect(backupSaveStatus({ kind: "refused", reason: "noRecoveryKit" }, "file", translate)?.kind).toBe(
      "error",
    );
  });
});
