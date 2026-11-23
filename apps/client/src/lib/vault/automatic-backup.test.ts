import { describe, expect, it } from "vitest";
import {
  automaticBackupSchedule,
  parseAutomaticBackupStatus,
  parseAutomaticBackupUpdate,
  type AutomaticBackupStatus,
} from "./automatic-backup";

const wire = {
  enabled: true,
  interval: "daily",
  folder: { label: "/media/disk/Ganbaru Backup", custom: false },
  lastBackupAt: "2026-11-22T09:00:00Z",
  nextBackupAt: "2026-11-23T09:00:00Z",
  failure: null,
  running: false,
};

function status(overrides: Partial<AutomaticBackupStatus>): AutomaticBackupStatus {
  return { ...parseAutomaticBackupStatus(wire), ...overrides };
}

describe("parseAutomaticBackupStatus", () => {
  it("reads a complete status", () => {
    expect(parseAutomaticBackupStatus(wire)).toEqual({ ...wire, failure: null });
  });

  it("reads a failure and absent times", () => {
    const parsed = parseAutomaticBackupStatus({
      ...wire,
      enabled: false,
      lastBackupAt: null,
      nextBackupAt: null,
      failure: { kind: "folderUnavailable", at: "2026-11-22T10:00:00Z" },
    });
    expect(parsed.failure).toEqual({ kind: "folderUnavailable", at: "2026-11-22T10:00:00Z" });
    expect(parsed.lastBackupAt).toBeNull();
  });

  it("rejects unknown intervals, failures, and malformed times", () => {
    expect(() => parseAutomaticBackupStatus({ ...wire, interval: "hourly" })).toThrow("interval");
    expect(() => parseAutomaticBackupStatus({ ...wire, failure: { kind: "disk", at: wire.lastBackupAt } })).toThrow(
      "failure",
    );
    expect(() => parseAutomaticBackupStatus({ ...wire, failure: { kind: "failed" } })).toThrow("no time");
    expect(() => parseAutomaticBackupStatus({ ...wire, lastBackupAt: "yesterday" })).toThrow("not a time");
  });

  it("rejects missing fields and an empty folder label", () => {
    expect(() => parseAutomaticBackupStatus({ ...wire, running: undefined })).toThrow("incomplete");
    expect(() => parseAutomaticBackupStatus({ ...wire, folder: { label: "", custom: true } })).toThrow("folder");
    expect(() => parseAutomaticBackupStatus(null)).toThrow("not an object");
  });
});

describe("parseAutomaticBackupUpdate", () => {
  it("reads saved and refused updates", () => {
    expect(parseAutomaticBackupUpdate({ kind: "saved", status: wire })).toEqual({
      kind: "saved",
      status: parseAutomaticBackupStatus(wire),
    });
    expect(parseAutomaticBackupUpdate({ kind: "refused", reason: "insideVault" })).toEqual({
      kind: "refused",
      reason: "insideVault",
    });
  });

  it("rejects unknown kinds and reasons", () => {
    expect(() => parseAutomaticBackupUpdate({ kind: "cancelled" })).toThrow("kind");
    expect(() => parseAutomaticBackupUpdate({ kind: "refused", reason: "busy" })).toThrow("refusal");
  });
});

describe("automaticBackupSchedule", () => {
  const now = new Date("2026-11-22T12:00:00Z");

  it("shows a running backup first", () => {
    expect(automaticBackupSchedule(status({ running: true }), now)).toEqual({ kind: "running" });
  });

  it("shows the last and next backups while on", () => {
    expect(automaticBackupSchedule(status({}), now)).toEqual({
      kind: "next",
      last: new Date("2026-11-22T09:00:00Z"),
      next: new Date("2026-11-23T09:00:00Z"),
    });
  });

  it("shows a due backup as soon", () => {
    expect(automaticBackupSchedule(status({ lastBackupAt: null, nextBackupAt: "2026-11-22T12:00:00Z" }), now)).toEqual({
      kind: "soon",
      last: null,
    });
  });

  it("shows only the last backup while off", () => {
    expect(automaticBackupSchedule(status({ enabled: false, nextBackupAt: null }), now)).toEqual({
      kind: "last",
      last: new Date("2026-11-22T09:00:00Z"),
    });
    expect(automaticBackupSchedule(status({ enabled: false, nextBackupAt: null, lastBackupAt: null }), now)).toEqual({
      kind: "none",
    });
  });
});
