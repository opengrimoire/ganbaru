import { describe, expect, it } from "vitest";
import type { Contact } from "$lib/api/contacts";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import { formatContactTrustDetail, formatTrustSummary, contactsMutationErrorKey } from "./presentation";

const t: Translate = ((key, ...args) => translateFromPartialCatalog({}, key, ...args)) as Translate;
const now = new Date("2026-10-08T12:00:00.000Z");

const contact: Contact = {
  id: "contact-1",
  publicKey: "k",
  displayName: "Ada",
  color: 1,
  state: "active",
  inviteTrust: "seven_days",
  inviteTrustExpiresAt: "2026-10-11T12:00:00.000Z",
  messageTrust: "until_revoked",
  messageTrustExpiresAt: null,
  acceptedAt: null,
  blockedAt: null,
  revision: 1,
  createdAt: "2026-10-01T00:00:00.000Z",
  updatedAt: "2026-10-01T00:00:00.000Z",
};

describe("contactsMutationErrorKey", () => {
  it("maps ownership and revision failures to their own sentences", () => {
    expect(contactsMutationErrorKey("revision_conflict")).toBe("revisionConflict");
    expect(contactsMutationErrorKey("read_only")).toBe("readOnly");
    expect(contactsMutationErrorKey("identity_unavailable")).toBe("readOnly");
    expect(contactsMutationErrorKey("invalid_card")).toBe("failed");
  });
});

describe("trust presentation", () => {
  it("phrases timed grants with the relative day formatter", () => {
    expect(formatTrustSummary(t, { kind: "until", expiresAt: "2026-10-11T12:00:00.000Z" }, now)).toBe("until in 3 days");
    expect(formatTrustSummary(t, { kind: "expired" }, now)).toBe("expired");
  });

  it("combines both scopes into one row detail", () => {
    expect(formatContactTrustDetail(t, contact, now)).toBe("Invite until in 3 days · Message always");
  });
});
