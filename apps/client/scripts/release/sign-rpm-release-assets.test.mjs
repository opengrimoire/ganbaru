import { describe, expect, it } from "vitest";
import { checksigConfirmsSignature, rpmSignArguments } from "./sign-rpm-release-assets.mjs";

const RPM = "/work/dist/release-assets/ganbaru-0.1.13-1.x86_64.rpm";

describe("RPM signing", () => {
  it("signs with the imported key and passes the passphrase without a prompt", () => {
    const args = rpmSignArguments(
      { keyId: "ABCDEF0123456789", gnupgHome: "/tmp/gnupg", passphraseFile: "/tmp/passphrase" },
      RPM,
    );

    expect(args[0]).toBe("--addsign");
    expect(args.at(-1)).toBe(RPM);
    expect(args).toContain("_gpg_name ABCDEF0123456789");
    expect(args).toContain("_gpg_path /tmp/gnupg");
    expect(args).toContain(
      "_gpg_sign_cmd_extra_args --batch --pinentry-mode loopback --passphrase-file /tmp/passphrase",
    );
  });

  it("accepts only a checksig report with a valid signature for the same package", () => {
    expect(checksigConfirmsSignature(`${RPM}: digests signatures OK\n`, RPM)).toBe(true);
    expect(checksigConfirmsSignature(`${RPM}: digests OK\n`, RPM)).toBe(false);
    expect(checksigConfirmsSignature(`${RPM}: digests SIGNATURES NOT OK\n`, RPM)).toBe(false);
    expect(checksigConfirmsSignature(`/other.rpm: digests signatures OK\n`, RPM)).toBe(false);
    expect(checksigConfirmsSignature("", RPM)).toBe(false);
  });
});
