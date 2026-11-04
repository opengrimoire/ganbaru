import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import {
  checksumViolations,
  formatChecksumManifest,
  parseChecksumManifest,
  planManifestPrune,
  verifyDetachedSignature,
} from "./release-checksums.mjs";

const DEB = "a".repeat(64);
const RPM = "b".repeat(64);
const OTHER = "c".repeat(64);

describe("checksum manifests", () => {
  it("round-trip sorted sha256sum text", () => {
    const text = formatChecksumManifest(new Map([["z.rpm", RPM], ["a.deb", DEB]]));
    expect(text).toBe(`${DEB}  a.deb\n${RPM}  z.rpm\n`);
    expect(parseChecksumManifest(text)).toEqual(new Map([["a.deb", DEB], ["z.rpm", RPM]]));
    expect(parseChecksumManifest(`${DEB} *./a.deb\n`)).toEqual(new Map([["a.deb", DEB]]));
  });

  it("reject duplicate, escaping, and malformed entries", () => {
    expect(() => parseChecksumManifest(`${DEB}  a.deb\n${RPM}  a.deb\n`)).toThrow(/Duplicate/u);
    expect(() => parseChecksumManifest(`${DEB}  ../a.deb\n`)).toThrow(/escapes/u);
    expect(() => parseChecksumManifest(`${DEB}  /a.deb\n`)).toThrow(/escapes/u);
    expect(() => parseChecksumManifest(`${DEB.toUpperCase()}  a.deb\n`)).toThrow(/Malformed/u);
  });

  it("flag a replaced asset and an asset the signed manifest never listed", () => {
    const manifest = new Map([["ganbaru.deb", DEB]]);
    expect(checksumViolations(manifest, new Map([["ganbaru.deb", DEB]]))).toEqual([]);
    expect(checksumViolations(manifest, new Map([["ganbaru.deb", OTHER]]))).toEqual([
      "ganbaru.deb: digest differs from the signed checksum manifest",
    ]);
    expect(checksumViolations(manifest, new Map([["ganbaru-renamed.deb", DEB]]))).toEqual([
      "ganbaru-renamed.deb: not listed in the signed checksum manifest",
    ]);
  });
});

describe("pool prune plan", () => {
  it("keeps only files the signed manifest vouches for with the same digest", () => {
    const manifest = new Map([
      ["apt/pool/main/g/ganbaru/old.deb", DEB],
      ["rpm/x86_64/old.rpm", RPM],
    ]);
    const observed = new Map([
      ["apt/pool/main/g/ganbaru/old.deb", DEB],
      ["rpm/x86_64/old.rpm", OTHER],
      ["apt/pool/main/x/evil/evil.deb", OTHER],
      ["rpm/link.rpm", ""],
    ]);
    expect(planManifestPrune(manifest, observed)).toEqual({
      keep: ["apt/pool/main/g/ganbaru/old.deb"],
      drop: ["apt/pool/main/x/evil/evil.deb", "rpm/link.rpm", "rpm/x86_64/old.rpm"],
    });
  });

  it("keeps nothing on the first publication", () => {
    expect(planManifestPrune(new Map(), new Map([["rpm/x86_64/old.rpm", RPM]]))).toEqual({
      keep: [],
      drop: ["rpm/x86_64/old.rpm"],
    });
  });
});

const gpgAvailable = spawnSync("gpg", ["--version"]).status === 0 && spawnSync("gpgv", ["--version"]).status === 0;

describe.skipIf(!gpgAvailable)("detached signature verification", () => {
  let home = "";

  /** Creates an unprotected throwaway key and returns its armored public key. */
  function generateKey(directory, uid) {
    const run = (args) => {
      const result = spawnSync("gpg", ["--batch", "--quiet", "--homedir", directory, ...args], { encoding: "utf8" });
      if (result.status !== 0) throw new Error(result.stderr);
      return result.stdout;
    };
    run(["--passphrase", "", "--quick-gen-key", uid, "ed25519", "sign", "never"]);
    return { publicKey: run(["--armor", "--export", uid]), run };
  }

  beforeAll(() => {
    home = mkdtempSync(path.join(tmpdir(), "ganbaru-checksum-test-"));
  });

  afterAll(() => {
    rmSync(home, { recursive: true, force: true });
  });

  it("accepts only the trusted key's signature over unchanged content", () => {
    const trustedHome = path.join(home, "trusted");
    const otherHome = path.join(home, "other");
    for (const directory of [trustedHome, otherHome]) {
      mkdirSync(directory, { mode: 0o700 });
    }
    const trusted = generateKey(trustedHome, "release@example.invalid");
    const other = generateKey(otherHome, "other@example.invalid");
    const manifest = path.join(home, "SHA256SUMS");
    const signature = `${manifest}.asc`;
    writeFileSync(manifest, `${DEB}  ganbaru.deb\n`);
    trusted.run(["--yes", "--detach-sign", "--armor", "-o", signature, manifest]);

    expect(() => verifyDetachedSignature(trusted.publicKey, manifest, signature)).not.toThrow();
    expect(() => verifyDetachedSignature(other.publicKey, manifest, signature)).toThrow(/not a valid signature/u);

    writeFileSync(manifest, `${OTHER}  ganbaru.deb\n`);
    expect(() => verifyDetachedSignature(trusted.publicKey, manifest, signature)).toThrow(/not a valid signature/u);

    other.run(["--yes", "--detach-sign", "--armor", "-o", signature, manifest]);
    expect(() => verifyDetachedSignature(trusted.publicKey, manifest, signature)).toThrow(/not a valid signature/u);
  });
});
