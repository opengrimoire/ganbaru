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
    expect(formatHandoffError(
      "Close running Chat terminals before moving or replacing this vault",
      t,
    )).toBe("vaultHandoff.blockerTerminal");
  });

  it("turns coordinator failures into same-network retry guidance", () => {
    expect(formatHandoffError(new Error("connect coordinator: refused"), t)).toBe(
      "vaultHandoff.unreachable",
    );
  });

  it("explains the ownership churn limit instead of a connectivity failure", () => {
    const message = "vault ownership changed too often; wait before switching devices again";
    expect(formatHandoffError(message, t)).toBe("vaultHandoff.ownershipChurnLimit");
    expect(formatOwnershipHandoffError(new Error(message), t)).toBe(
      "vaultHandoff.ownershipChurnLimit",
    );
  });

  it("explains the per-device snapshot limit instead of a connectivity failure", () => {
    const message =
      "this device requested the vault too often; wait before refreshing or switching again";
    expect(formatHandoffError(message, t)).toBe("vaultHandoff.bundleRequestLimit");
    expect(formatOwnershipHandoffError(new Error(message), t)).toBe(
      "vaultHandoff.bundleRequestLimit",
    );
  });

  it("explains disabled linking instead of a connectivity or transfer failure", () => {
    const message =
      "vault linking is disabled because its saved state could not be loaded: read pairing state: network transfer is active";
    expect(formatHandoffError(message, t)).toBe("vaultHandoff.linkingDisabled");
    expect(formatOwnershipHandoffError(new Error(message), t)).toBe(
      "vaultHandoff.linkingDisabled",
    );
  });

  it("explains a missing or unreadable device linking key instead of a connectivity failure", () => {
    const missing =
      "this device's linking key is missing from the system keyring; unlink this device and link it again";
    expect(formatHandoffError(missing, t)).toBe("vaultHandoff.deviceKeyMissing");
    const unavailable =
      "this device's linking key could not be read from the system keyring: secret service connection unavailable";
    expect(formatHandoffError(unavailable, t)).toBe("vaultHandoff.deviceKeyUnavailable");
    expect(formatOwnershipHandoffError(new Error(unavailable), t)).toBe(
      "vaultHandoff.deviceKeyUnavailable",
    );
  });

  it("explains a busy coordinator instead of a connectivity failure", () => {
    const message = "the coordinator is handling too many connections; retry shortly";
    expect(formatHandoffError(message, t)).toBe("vaultHandoff.coordinatorBusy");
    expect(formatOwnershipHandoffError(new Error(message), t)).toBe(
      "vaultHandoff.coordinatorBusy",
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

  it("explains that handoff pauses while another vault is open", () => {
    expect(formatHandoffError("the linked vault is not active on this device", t))
      .toBe("vaultHandoff.linkedVaultInactive");
    expect(formatHandoffError("uploaded vault is not active on the coordinator", t))
      .toBe("vaultHandoff.coordinatorVaultInactive");
    expect(formatHandoffError("requested vault is not active on the coordinator", t))
      .toBe("vaultHandoff.coordinatorVaultInactive");
  });

  it("explains each way a coordinator can leave an enrollment unapproved", () => {
    expect(formatHandoffError("the coordinator declined this device", t))
      .toBe("vaultHandoff.enrollmentDeclined");
    expect(formatHandoffError("the coordinator did not approve this device in time", t))
      .toBe("vaultHandoff.enrollmentUnanswered");
    expect(formatHandoffError("the coordinator did not answer the enrollment", t))
      .toBe("vaultHandoff.enrollmentUnanswered");
    expect(formatHandoffError("another device is waiting for approval on the coordinator", t))
      .toBe("vaultHandoff.enrollmentBusy");
    expect(formatHandoffError("this device is no longer waiting for approval", t))
      .toBe("vaultHandoff.enrollmentGone");
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

  it("asks for the new code when the coordinator replaced the one in use", () => {
    expect(formatPairingCodeError("the pairing code on the coordinator was replaced", t))
      .toBe("vaultHandoff.enrollmentReplaced");
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
