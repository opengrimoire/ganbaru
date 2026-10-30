import { describe, expect, it } from "vitest";
import { SYNC_SERVER_ERROR_CODES, SyncServerError, type SyncServerView } from "$lib/api/sync";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import { groupSyncFingerprint, syncServerErrorLabel, syncServerLines } from "./server";

const t: Translate = ((key, ...args) => translateFromPartialCatalog({}, key, ...args)) as Translate;

const relay: SyncServerView = {
  mode: "relay",
  endpoints: ["relay.example:43823", "10.0.0.2:43823"],
  certificateFingerprint: "ab12cd34ef",
  canManage: true,
  probe: { protocolVersion: 1, compatible: true, problem: null },
};

describe("server lines", () => {
  it("groups the fingerprint and keeps a short last group", () => {
    expect(groupSyncFingerprint("ab12cd34ef")).toBe("ab12 cd34 ef");
    expect(groupSyncFingerprint("")).toBe("");
  });

  it("lists addresses, certificate, and a reachable relay's protocol version", () => {
    expect(syncServerLines(t, relay)).toEqual([
      "Address: relay.example:43823, 10.0.0.2:43823",
      "Certificate: ab12 cd34 ef",
      "Reachable, protocol version 1",
    ]);
  });

  it("explains an incompatible relay once, without repeating the problem", () => {
    const lines = syncServerLines(t, {
      ...relay,
      probe: { protocolVersion: 2, compatible: false, problem: "incompatible" },
    });
    expect(lines.slice(2)).toEqual(["Reachable, but it uses protocol version 2. Update Ganbaru or the relay."]);
  });

  it("calls a relay incompatible when it refuses the protocol without naming a version", () => {
    const lines = syncServerLines(t, {
      ...relay,
      probe: { protocolVersion: null, compatible: false, problem: "incompatible" },
    });
    expect(lines.slice(2)).toEqual(["The relay uses another protocol version. Update Ganbaru or the relay."]);
  });

  it("adds the localized reason when the relay did not answer", () => {
    const lines = syncServerLines(t, {
      ...relay,
      probe: { protocolVersion: null, compatible: false, problem: "unreachable" },
    });
    expect(lines.slice(2)).toEqual([
      "Not reachable right now",
      "The relay could not be reached, or its certificate did not match.",
    ]);
  });

  it("shows no probe line for an anchor server", () => {
    expect(syncServerLines(t, { ...relay, mode: "anchor", endpoints: [], probe: null })).toEqual([
      "Certificate: ab12 cd34 ef",
    ]);
  });
});

describe("server error labels", () => {
  it("localizes every error code", () => {
    for (const code of SYNC_SERVER_ERROR_CODES) {
      const label = syncServerErrorLabel(t, new SyncServerError(code, null));
      expect(label).not.toContain("sync.server");
      expect(label.length).toBeGreaterThan(0);
    }
  });

  it("explains a relay refusal through its problem", () => {
    expect(syncServerErrorLabel(t, new SyncServerError("relay", "setupCodeRefused"))).toBe(
      "The relay refused the setup code. It may have expired or been used already; ask the relay for a new one.",
    );
    expect(syncServerErrorLabel(t, new SyncServerError("relay", null))).toBe("The server could not be changed. Try again.");
  });
});
