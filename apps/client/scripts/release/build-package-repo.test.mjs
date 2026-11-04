import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { APT_RELEASE_VALID_DAYS, aptReleaseConfig } from "./build-package-repo.mjs";

const SECONDS_PER_DAY = 24 * 60 * 60;

describe("apt Release configuration", () => {
  it("bounds how long a signed Release stays valid", () => {
    expect(aptReleaseConfig(APT_RELEASE_VALID_DAYS)).toContain(
      `APT::FTPArchive::Release::ValidTime "${APT_RELEASE_VALID_DAYS * SECONDS_PER_DAY}";`,
    );
    expect(() => aptReleaseConfig(0)).toThrow(/positive/u);
    expect(() => aptReleaseConfig(1.5)).toThrow(/positive/u);
  });

  const aptFtparchiveAvailable = spawnSync("apt-ftparchive", ["--help"]).status === 0;

  it.skipIf(!aptFtparchiveAvailable)("makes apt-ftparchive write Valid-Until after Date", () => {
    const root = mkdtempSync(path.join(tmpdir(), "ganbaru-apt-release-"));
    try {
      const config = path.join(root, "release.conf");
      mkdirSync(path.join(root, "dists", "stable", "main", "binary-amd64"), { recursive: true });
      writeFileSync(path.join(root, "dists", "stable", "main", "binary-amd64", "Packages"), "");
      writeFileSync(config, aptReleaseConfig(APT_RELEASE_VALID_DAYS));
      const result = spawnSync("apt-ftparchive", ["-c", config, "release", "dists/stable"], {
        cwd: root,
        encoding: "utf8",
      });
      expect(result.status).toBe(0);

      const field = (name) => new RegExp(`^${name}: (.+)$`, "mu").exec(result.stdout)?.[1];
      const date = Date.parse(field("Date") ?? "");
      const validUntil = Date.parse(field("Valid-Until") ?? "");
      expect(Number.isNaN(validUntil)).toBe(false);
      expect((validUntil - date) / 1000).toBe(APT_RELEASE_VALID_DAYS * SECONDS_PER_DAY);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
});
