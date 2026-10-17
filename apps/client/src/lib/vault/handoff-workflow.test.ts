import { describe, expect, it } from "vitest";
import type { Translate } from "$lib/i18n/translator.svelte";
import {
  formatHandoffError,
  formatPairingCodeError,
  formatOwnershipHandoffError,
  hasNewLinkedDevice,
} from "./handoff-workflow";

const t = ((key: string, value?: string) => value ? `${key}:${value}` : key) as Translate;

describe("formatHandoffError", () => {
  it("turns ownership blockers into direct recovery guidance", () => {
    expect(formatHandoffError("active Chat work blocks vault handoff", t)).toBe(
      "vaultHandoff.blockerChat",
    );
    expect(formatHandoffError("active Pomodoro run blocks handoff", t)).toBe(
      "vaultHandoff.blockerFocus",
    );
  });

  it("turns coordinator failures into same-network retry guidance", () => {
    expect(formatHandoffError(new Error("connect coordinator: refused"), t)).toBe(
      "vaultHandoff.unreachable",
    );
  });

  it("identifies an unreachable main device during an ownership request", () => {
    expect(formatOwnershipHandoffError(
      new Error("the current vault owner is unreachable"),
      t,
    )).toBe("vaultHandoff.ownerUnreachable");
  });

  it("turns schema mismatches into update guidance", () => {
    expect(formatHandoffError(
      "handoff compatibility mismatch between Ganbaru 1.0 and 2.0",
      t,
    )).toBe("vaultHandoff.incompatibleVersions");
    expect(formatHandoffError(
      "handoff protocol version is unsupported",
      t,
    )).toBe("vaultHandoff.incompatibleVersions");
  });

  it("explains that another coordinator must be unlinked first", () => {
    expect(formatHandoffError(
      "this device is already linked to another coordinator",
      t,
    )).toBe("vaultHandoff.differentCoordinator");
  });
});

describe("formatPairingCodeError", () => {
  it("gives useful feedback for malformed and expired codes", () => {
    expect(formatPairingCodeError(
      "decode pairing invitation: expected value at line 1 column 1", t,
    )).toBe("vaultHandoff.codeInvalid");
    expect(formatPairingCodeError(
      "pairing invitation has expired", t,
    )).toBe("vaultHandoff.codeExpired");
  });

  it("preserves guidance for connection failures", () => {
    expect(formatPairingCodeError("connect coordinator: refused", t)).toBe(
      "vaultHandoff.unreachable",
    );
  });
});

describe("hasNewLinkedDevice", () => {
  it("keeps an invitation open while the existing membership is unchanged", () => {
    expect(hasNewLinkedDevice(
      new Set(["phone-1"]),
      [{ deviceId: "phone-1" }],
    )).toBe(false);
  });

  it("detects the device enrolled through the active invitation", () => {
    expect(hasNewLinkedDevice(
      new Set(["phone-1"]),
      [{ deviceId: "phone-1" }, { deviceId: "laptop-2" }],
    )).toBe(true);
  });
});
