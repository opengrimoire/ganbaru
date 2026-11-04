import { describe, expect, it } from "vitest";
import {
  parseAvailableUpdate,
  parseUpdateDownloadEvent,
  parseUpdateInstallContext,
  parseStoredUpdateCheckAt,
  releasePageUrl,
  shouldRunAutomaticUpdateCheck,
  UPDATE_AUTO_CHECK_INTERVAL_MS,
  updatePrimaryAction,
} from "./update-policy";

describe("automatic update checks", () => {
  it("runs a startup check when enabled even if a previous check exists", () => {
    const now = Date.UTC(2026, 0, 2);
    const recent = new Date(now - 1_000).toISOString();

    expect(shouldRunAutomaticUpdateCheck(true, recent, now, "startup")).toBe(true);
  });

  it("runs a periodic check when enabled and no previous check exists", () => {
    expect(shouldRunAutomaticUpdateCheck(true, null, Date.UTC(2026, 0, 2))).toBe(true);
  });

  it("does not run when disabled", () => {
    expect(shouldRunAutomaticUpdateCheck(false, null, Date.UTC(2026, 0, 2))).toBe(false);
  });

  it("waits until the daily interval has elapsed for periodic checks", () => {
    const now = Date.UTC(2026, 0, 2);
    const recent = new Date(now - UPDATE_AUTO_CHECK_INTERVAL_MS + 1).toISOString();
    const old = new Date(now - UPDATE_AUTO_CHECK_INTERVAL_MS).toISOString();

    expect(shouldRunAutomaticUpdateCheck(true, recent, now)).toBe(false);
    expect(shouldRunAutomaticUpdateCheck(true, old, now)).toBe(true);
  });

  it("runs when the stored timestamp is invalid or in the future", () => {
    const now = Date.UTC(2026, 0, 2);
    expect(shouldRunAutomaticUpdateCheck(true, "invalid", now)).toBe(true);
    expect(
      shouldRunAutomaticUpdateCheck(
        true,
        new Date(now + UPDATE_AUTO_CHECK_INTERVAL_MS).toISOString(),
        now,
      ),
    ).toBe(true);
  });
});

describe("update helpers", () => {
  it("normalizes valid stored timestamps", () => {
    expect(parseStoredUpdateCheckAt("2026-01-02T03:04:05.000Z")).toBe(
      "2026-01-02T03:04:05.000Z",
    );
    expect(parseStoredUpdateCheckAt("nope")).toBeNull();
    expect(parseStoredUpdateCheckAt(null)).toBeNull();
  });

  it("builds a GitHub release page URL from a valid updater version", () => {
    expect(releasePageUrl("opengrimoire/ganbaru", "0.1.0")).toBe(
      "https://github.com/opengrimoire/ganbaru/releases/tag/app-v0.1.0",
    );
  });

  it("rejects release page inputs that should not open external URLs", () => {
    expect(releasePageUrl("opengrimoire/ganbaru", "../0.1.0")).toBeNull();
    expect(releasePageUrl("https://github.com/opengrimoire/ganbaru", "0.1.0")).toBeNull();
    expect(releasePageUrl("opengrimoire/ganbaru", null)).toBeNull();
  });

  it("parses a self-updater install context", () => {
    const context = parseUpdateInstallContext(
      {
        selfUpdater: true,
        packageManager: null,
        copyCommand: "sudo apt update",
        releasePageUrl: "https://github.com/opengrimoire/ganbaru/releases/latest",
      },
      "opengrimoire/ganbaru",
    );

    expect(context.selfUpdater).toBe(true);
    expect(context.packageManager).toBeNull();
    expect(context.copyCommand).toBeNull();
    expect(updatePrimaryAction(context)).toBe("self-updater");
  });

  it("parses a package-manager install context with a command", () => {
    const context = parseUpdateInstallContext(
      {
        selfUpdater: false,
        packageManager: "apt",
        copyCommand: " sudo apt update && sudo apt install ganbaru ",
        releasePageUrl: "https://github.com/opengrimoire/ganbaru/releases/latest",
      },
      "opengrimoire/ganbaru",
    );

    expect(context.packageManager).toBe("apt");
    expect(context.copyCommand).toBe("sudo apt update && sudo apt install ganbaru");
    expect(updatePrimaryAction(context)).toBe("copy-command");
  });

  it("falls back to the release page when install context is unusable", () => {
    const context = parseUpdateInstallContext(
      {
        selfUpdater: false,
        packageManager: "not-real",
        copyCommand: "",
        releasePageUrl: "https://example.com/releases/latest",
      },
      "opengrimoire/ganbaru",
    );

    expect(context.packageManager).toBeNull();
    expect(context.copyCommand).toBeNull();
    expect(context.releasePageUrl).toBe(
      "https://github.com/opengrimoire/ganbaru/releases/latest",
    );
    expect(updatePrimaryAction(context)).toBe("release-page");
  });
});

describe("Rust updater payloads", () => {
  it("accepts an offered update and drops a non-string body", () => {
    expect(parseAvailableUpdate({ version: "0.1.13", currentVersion: "0.1.12", body: "Notes" })).toEqual({
      version: "0.1.13",
      currentVersion: "0.1.12",
      body: "Notes",
    });
    expect(parseAvailableUpdate({ version: "0.1.13", currentVersion: "0.1.12", body: 4 })?.body).toBeNull();
  });

  it("treats missing or malformed versions as no update", () => {
    expect(parseAvailableUpdate(null)).toBeNull();
    expect(parseAvailableUpdate({ version: "latest", currentVersion: "0.1.12" })).toBeNull();
    expect(parseAvailableUpdate({ version: "0.1.13" })).toBeNull();
  });

  it("parses progress events and rejects unusable byte counts", () => {
    expect(parseUpdateDownloadEvent({ event: "Started", data: { contentLength: 10 } })).toEqual({
      event: "Started",
      data: { contentLength: 10 },
    });
    expect(parseUpdateDownloadEvent({ event: "Started", data: { contentLength: null } })).toEqual({
      event: "Started",
      data: { contentLength: null },
    });
    expect(parseUpdateDownloadEvent({ event: "Progress", data: { chunkLength: 4 } })).toEqual({
      event: "Progress",
      data: { chunkLength: 4 },
    });
    expect(parseUpdateDownloadEvent({ event: "Finished" })).toEqual({ event: "Finished" });
    expect(parseUpdateDownloadEvent({ event: "Progress", data: { chunkLength: -1 } })).toBeNull();
    expect(parseUpdateDownloadEvent({ event: "Progress", data: { chunkLength: 1.5 } })).toBeNull();
    expect(parseUpdateDownloadEvent({ event: "Other", data: {} })).toBeNull();
    expect(parseUpdateDownloadEvent("Finished")).toBeNull();
  });
});
