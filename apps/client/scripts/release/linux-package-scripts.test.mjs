import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const PACKAGING_DIR = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "..",
  "src-tauri",
  "packaging",
);
const DEB_POSTINST = path.join(PACKAGING_DIR, "deb", "postinst");
const RPM_POSTINSTALL = path.join(PACKAGING_DIR, "rpm", "postinstall.sh");
const SOURCE_KEY = "usr/lib/ganbaru/package-repo/ganbaru-package-repo.asc";
const APT_SOURCE = "etc/apt/sources.list.d/ganbaru.sources";
const DNF_REPO = "etc/yum.repos.d/ganbaru.repo";
const ZYPPER_REPO = "etc/zypp/repos.d/ganbaru.repo";

/** The scripts need a POSIX shell and GNU `install -D`. */
const posixShellAvailable = process.platform === "linux";

describe.skipIf(!posixShellAvailable)("Linux package repository scripts", () => {
  let root = "";

  /**
   * Runs a maintainer script with every absolute `/etc/` and `/usr/` path moved under the test
   * root, so it touches only the temporary tree.
   */
  function runScript(scriptPath, args) {
    const rooted = readFileSync(scriptPath, "utf8").replace(/(?<![\w:/.])\/(etc|usr)\//gu, `${root}/$1/`);
    const copy = path.join(root, path.basename(scriptPath));
    writeFileSync(copy, rooted);
    const result = spawnSync("sh", [copy, ...args], { encoding: "utf8" });
    expect(result.stderr).toBe("");
    expect(result.status).toBe(0);
  }

  const file = (relative) => path.join(root, relative);
  const read = (relative) => readFileSync(file(relative), "utf8");
  const write = (relative, text) => {
    mkdirSync(path.dirname(file(relative)), { recursive: true });
    writeFileSync(file(relative), text);
  };

  beforeEach(() => {
    root = mkdtempSync(path.join(tmpdir(), "ganbaru-package-scripts-"));
    write(SOURCE_KEY, "-----BEGIN PGP PUBLIC KEY BLOCK-----\n");
  });

  afterEach(() => {
    rmSync(root, { recursive: true, force: true });
  });

  it("enables the apt source on a first install and keeps the key current", () => {
    runScript(DEB_POSTINST, ["configure", ""]);
    rmSync(file(APT_SOURCE));
    runScript(DEB_POSTINST, ["configure"]);

    expect(read(APT_SOURCE)).toContain("Enabled: yes\n");
    expect(read(APT_SOURCE)).toContain(`Signed-By: ${root}/usr/share/keyrings/ganbaru-package-repo.asc\n`);
    expect(read("usr/share/keyrings/ganbaru-package-repo.asc")).toBe(read(SOURCE_KEY));
  });

  it("keeps a disabled apt source disabled and a removed one removed on upgrade", () => {
    write(APT_SOURCE, "Types: deb\nURIs: https://example.invalid\nEnabled: no\n");
    runScript(DEB_POSTINST, ["configure", "0.1.12"]);
    expect(read(APT_SOURCE)).toContain("Enabled: no\n");
    expect(read(APT_SOURCE)).toContain("URIs: https://opengrimoire.github.io/ganbaru/packages/apt\n");

    write(APT_SOURCE, "Types: deb\nEnabled: false\n");
    runScript(DEB_POSTINST, ["configure", "0.1.12"]);
    expect(read(APT_SOURCE)).toContain("Enabled: no\n");

    rmSync(file(APT_SOURCE));
    runScript(DEB_POSTINST, ["configure", "0.1.12"]);
    expect(existsSync(file(APT_SOURCE))).toBe(false);
    expect(existsSync(file("usr/share/keyrings/ganbaru-package-repo.asc"))).toBe(true);
  });

  it("writes no source outside configure", () => {
    runScript(DEB_POSTINST, ["abort-upgrade", "0.1.13"]);
    expect(existsSync(file(APT_SOURCE))).toBe(false);
  });

  it("requires package signatures for dnf and zypper on a first install", () => {
    mkdirSync(file("etc/zypp"), { recursive: true });
    runScript(RPM_POSTINSTALL, ["1"]);

    for (const repo of [DNF_REPO, ZYPPER_REPO]) {
      const text = read(repo);
      expect(text).toContain("enabled=1\n");
      expect(text).toContain("gpgcheck=1\n");
      expect(text).toContain("repo_gpgcheck=1\n");
      expect(text).not.toContain("gpgcheck=0");
    }
    expect(read("etc/pki/rpm-gpg/RPM-GPG-KEY-ganbaru")).toBe(read(SOURCE_KEY));
  });

  it("keeps each disabled or removed RPM repository file as the user left it on upgrade", () => {
    mkdirSync(file("etc/zypp/repos.d"), { recursive: true });
    write(DNF_REPO, "[ganbaru]\nenabled = 0\ngpgcheck=0\n");
    runScript(RPM_POSTINSTALL, ["2"]);

    expect(read(DNF_REPO)).toContain("enabled=0\n");
    expect(read(DNF_REPO)).toContain("gpgcheck=1\n");
    expect(existsSync(file(ZYPPER_REPO))).toBe(false);

    write(ZYPPER_REPO, "[ganbaru]\nenabled=1\n");
    runScript(RPM_POSTINSTALL, ["2"]);
    expect(read(ZYPPER_REPO)).toContain("enabled=1\n");
    expect(read(DNF_REPO)).toContain("enabled=0\n");

    write(ZYPPER_REPO, "[ganbaru]\nenabled=off\n");
    runScript(RPM_POSTINSTALL, ["2"]);
    expect(read(ZYPPER_REPO)).toContain("enabled=0\n");
  });
});
