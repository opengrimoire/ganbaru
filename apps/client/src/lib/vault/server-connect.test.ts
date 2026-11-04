import { describe, expect, it } from "vitest";
import type { Translate } from "$lib/i18n/translator.svelte";
import { isServerWithoutVault, serverConnectError } from "./server-connect";

const t = ((key: string) => key) as Translate;
const errors = "vaultHandoff.serverConnect.errors";

describe("serverConnectError", () => {
  it.each([
    ["anchor setup code is invalid", `${errors}.code`],
    ["the server refused this setup code", `${errors}.code`],
    ["read the recovery kit: invalid length", `${errors}.kit`],
    ["the server rejected the recovery proposal with code 7", `${errors}.rejected`],
    ["the server rejected the enrollment with code 3", `${errors}.joinRejected`],
    ["the server holds this enrollment until the vault's anchor is online", `${errors}.waiting`],
    ["the vault on this server does not enroll this device", `${errors}.notEnrolled`],
    ["this server runs no vault yet", `${errors}.noVault`],
    ["this vault is already open on this device", `${errors}.alreadyOpen`],
    ["this recovery kit does not belong to the vault on this server", `${errors}.wrongKit`],
    ["could not reach the server: connecting to 192.168.1.20:43823 timed out", `${errors}.unreachable`],
  ])("names what to fix for %s", (cause, expected) => {
    expect(serverConnectError(cause, t)).toBe(expected);
    expect(serverConnectError(new Error(cause), t)).toBe(expected);
  });

  it("never shows native text it does not recognize", () => {
    expect(serverConnectError("connection reset near SECRET", t)).toBe(`${errors}.failed`);
    expect(serverConnectError(null, t)).toBe(`${errors}.failed`);
  });
});

describe("isServerWithoutVault", () => {
  it("recognizes an empty server in text and errors", () => {
    expect(isServerWithoutVault("this server runs no vault yet")).toBe(true);
    expect(isServerWithoutVault(new Error("the server runs no vault yet"))).toBe(true);
  });

  it("leaves every other refusal to the error message", () => {
    expect(isServerWithoutVault("the server refused this setup code")).toBe(false);
    expect(isServerWithoutVault("could not reach the server: timed out")).toBe(false);
    expect(isServerWithoutVault(null)).toBe(false);
  });
});
