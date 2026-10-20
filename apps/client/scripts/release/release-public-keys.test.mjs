import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterAll, describe, expect, it } from "vitest";
import {
  PACKAGE_REPO_PUBLIC_KEY_ENV,
  PACKAGE_REPO_PUBLIC_KEY_FILE,
  UPDATER_PUBLIC_KEY_ENV,
  UPDATER_PUBLIC_KEY_FILE,
  packageRepoPublicKey,
  updaterPublicKey,
} from "./release-public-keys.mjs";

const scratch = mkdtempSync(path.join(tmpdir(), "ganbaru-release-keys-"));

afterAll(() => rmSync(scratch, { recursive: true, force: true }));

/** Writes a scratch key file and returns its path. */
function keyFile(name, text) {
  const filePath = path.join(scratch, name);
  writeFileSync(filePath, text, "utf8");
  return filePath;
}

describe("committed release public keys", () => {
  it("are read from the repository without any environment", () => {
    const updater = updaterPublicKey({});
    expect(updater).toBe(readFileSync(UPDATER_PUBLIC_KEY_FILE, "utf8").trim());
    expect(Buffer.from(updater, "base64").toString("utf8")).toMatch(/^untrusted comment: minisign public key: /u);

    const packageKey = packageRepoPublicKey({});
    expect(packageKey).toBe(readFileSync(PACKAGE_REPO_PUBLIC_KEY_FILE, "utf8"));
    expect(packageKey.startsWith("-----BEGIN PGP PUBLIC KEY BLOCK-----\n")).toBe(true);
    expect(packageKey.endsWith("-----END PGP PUBLIC KEY BLOCK-----\n")).toBe(true);
  });

  it("accept a configured copy only when it matches the committed key", () => {
    const updater = updaterPublicKey({});
    expect(updaterPublicKey({ [UPDATER_PUBLIC_KEY_ENV]: `  ${updater}\n` })).toBe(updater);
    expect(() => updaterPublicKey({ [UPDATER_PUBLIC_KEY_ENV]: `${updater.slice(0, -4)}AAA=` })).toThrow(
      /differs from the committed key/u,
    );

    const packageKey = packageRepoPublicKey({});
    const crlfCopy = packageKey.replace(/\n/gu, "\r\n");
    expect(packageRepoPublicKey({ [PACKAGE_REPO_PUBLIC_KEY_ENV]: crlfCopy })).toBe(packageKey);
    const otherKey = packageKey.replace(/\n[A-Za-z0-9+/]{8}/u, (line) => `\n${"A".repeat(line.length - 1)}`);
    expect(() => packageRepoPublicKey({ [PACKAGE_REPO_PUBLIC_KEY_ENV]: otherKey })).toThrow(
      /differs from the committed key/u,
    );
  });

  it("treat an empty configured copy as unset", () => {
    expect(updaterPublicKey({ [UPDATER_PUBLIC_KEY_ENV]: " " })).toBe(updaterPublicKey({}));
    expect(packageRepoPublicKey({ [PACKAGE_REPO_PUBLIC_KEY_ENV]: "" })).toBe(packageRepoPublicKey({}));
  });

  it("reject malformed committed keys", () => {
    const notMinisign = Buffer.from("untrusted comment: something else\nAAAA\n").toString("base64");
    expect(() => updaterPublicKey({}, keyFile("other.pub", notMinisign))).toThrow(/minisign public key/u);
    expect(() => updaterPublicKey({}, keyFile("text.pub", "not base64!"))).toThrow(/minisign public key/u);
    const shortKey = Buffer.from("untrusted comment: minisign public key: 1\nRWQ=\n").toString("base64");
    expect(() => updaterPublicKey({}, keyFile("short.pub", shortKey))).toThrow(/minisign public key/u);
    expect(() =>
      packageRepoPublicKey({}, keyFile("secret.asc", "-----BEGIN PGP PRIVATE KEY BLOCK-----\n-----END PGP PRIVATE KEY BLOCK-----\n")),
    ).toThrow(/ASCII-armored public key/u);
    expect(() => packageRepoPublicKey({}, path.join(scratch, "missing.asc"))).toThrow(/read committed release key/u);
  });
});
