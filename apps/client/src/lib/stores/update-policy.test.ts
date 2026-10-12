import { describe, expect, it } from "vitest";
import {
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
    expect(releasePageUrl("opengrimoire/ganbaru-ai", "0.1.0")).toBe(
      "https://github.com/opengrimoire/ganbaru-ai/releases/tag/app-v0.1.0",
    );
  });

  it("rejects release page inputs that should not open external URLs", () => {
    expect(releasePageUrl("opengrimoire/ganbaru-ai", "../0.1.0")).toBeNull();
    expect(releasePageUrl("https://github.com/opengrimoire/ganbaru-ai", "0.1.0")).toBeNull();
    expect(releasePageUrl("opengrimoire/ganbaru-ai", null)).toBeNull();
  });

  it("parses a self-updater install context", () => {
    const context = parseUpdateInstallContext(
      {
        selfUpdater: true,
        packageManager: null,
        copyCommand: "sudo apt update",
        releasePageUrl: "https://github.com/opengrimoire/ganbaru-ai/releases/latest",
      },
      "opengrimoire/ganbaru-ai",
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
        copyCommand: " sudo apt update && sudo apt install ganbaru-ai ",
        releasePageUrl: "https://github.com/opengrimoire/ganbaru-ai/releases/latest",
      },
      "opengrimoire/ganbaru-ai",
    );

    expect(context.packageManager).toBe("apt");
    expect(context.copyCommand).toBe("sudo apt update && sudo apt install ganbaru-ai");
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
      "opengrimoire/ganbaru-ai",
    );

    expect(context.packageManager).toBeNull();
    expect(context.copyCommand).toBeNull();
    expect(context.releasePageUrl).toBe(
      "https://github.com/opengrimoire/ganbaru-ai/releases/latest",
    );
    expect(updatePrimaryAction(context)).toBe("release-page");
  });
});
