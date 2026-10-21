import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const MANIFEST_PATH = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "..",
  "..",
  "..",
  "extensions",
  "chromium",
  "manifest.json",
);

/**
 * Permissions the extension uses. Tab URLs come from `tabs`, navigation events from
 * `webNavigation`, and the block page favicon from `favicon`; none of them need site access.
 */
const ALLOWED_PERMISSIONS = [
  "alarms",
  "favicon",
  "idle",
  "nativeMessaging",
  "storage",
  "tabs",
  "webNavigation",
];

describe("Chromium extension manifest", () => {
  const manifest = JSON.parse(readFileSync(MANIFEST_PATH, "utf8"));

  it("requests no access to page content", () => {
    expect(manifest.host_permissions).toBeUndefined();
    expect(manifest.optional_host_permissions).toBeUndefined();
    expect(manifest.content_scripts).toBeUndefined();
  });

  it("requires a browser with session storage, so tab URLs never fall back to disk", () => {
    expect(Number(manifest.minimum_chrome_version)).toBeGreaterThanOrEqual(102);
  });

  it("requests only the permissions its blocking paths use", () => {
    expect([...manifest.permissions].sort()).toEqual(ALLOWED_PERMISSIONS);
    expect(manifest.optional_permissions).toBeUndefined();
  });
});
