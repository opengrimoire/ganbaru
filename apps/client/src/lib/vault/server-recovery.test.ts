import { describe, expect, it } from "vitest";
import type { Translate } from "$lib/i18n/translator.svelte";
import { serverRecoveryError } from "./server-recovery";

const t = ((key: string) => key) as Translate;
const errors = "vaultHandoff.serverRecovery.errors";

describe("serverRecoveryError", () => {
  it.each([
    ["anchor setup code is invalid", `${errors}.code`],
    ["the server refused this setup code", `${errors}.code`],
    ["read the recovery kit: invalid length", `${errors}.kit`],
    ["the server rejected the recovery proposal with code 7", `${errors}.rejected`],
    ["this server runs no vault yet", `${errors}.noVault`],
    ["this vault is already open on this device", `${errors}.alreadyOpen`],
  ])("names what to fix for %s", (cause, expected) => {
    expect(serverRecoveryError(cause, t)).toBe(expected);
    expect(serverRecoveryError(new Error(cause), t)).toBe(expected);
  });

  it("never shows native text it does not recognize", () => {
    expect(serverRecoveryError("connection reset near SECRET", t)).toBe(`${errors}.failed`);
    expect(serverRecoveryError(null, t)).toBe(`${errors}.failed`);
  });
});
