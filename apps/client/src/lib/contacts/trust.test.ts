import { describe, expect, it } from "vitest";
import type { Contact } from "$lib/api/contacts";
import { isContactTrustDuration, CONTACT_TRUST_DURATIONS } from "$lib/stores/preference-options";
import { daysUntil, trustDurationForKind, trustKindForDuration, trustStatus } from "./trust";

const now = new Date("2026-10-08T12:00:00.000Z");

function contactWith(overrides: Partial<Contact>): Contact {
  return {
    id: "contact-1",
    publicKey: "k",
    displayName: "Ada",
    color: 1,
    state: "active",
    inviteTrust: "not_allowed",
    inviteTrustExpiresAt: null,
    messageTrust: "not_allowed",
    messageTrustExpiresAt: null,
    acceptedAt: null,
    blockedAt: null,
    revision: 1,
    createdAt: "2026-10-01T00:00:00.000Z",
    updatedAt: "2026-10-01T00:00:00.000Z",
    ...overrides,
  };
}

describe("trust duration mapping", () => {
  it("round-trips every UI duration through the wire kind", () => {
    for (const duration of CONTACT_TRUST_DURATIONS) {
      expect(trustDurationForKind(trustKindForDuration(duration))).toBe(duration);
    }
    expect(trustKindForDuration("sevenDays")).toBe("seven_days");
    expect(trustKindForDuration("untilRevoked")).toBe("until_revoked");
  });

  it("recognizes only the catalog durations", () => {
    expect(isContactTrustDuration("thirtyDays")).toBe(true);
    expect(isContactTrustDuration("thirty_days")).toBe(false);
    expect(isContactTrustDuration(30)).toBe(false);
  });
});

describe("trustStatus", () => {
  it("reports timed grants as active until they run out", () => {
    const contact = contactWith({ inviteTrust: "seven_days", inviteTrustExpiresAt: "2026-10-10T12:00:00.000Z" });
    expect(trustStatus(contact, "invite", now)).toEqual({ kind: "until", expiresAt: "2026-10-10T12:00:00.000Z" });
    expect(trustStatus(contact, "invite", new Date("2026-10-10T12:00:00.000Z"))).toEqual({ kind: "expired" });
  });

  it("treats a timed grant without an expiry as expired", () => {
    expect(trustStatus(contactWith({ messageTrust: "thirty_days" }), "message", now)).toEqual({ kind: "expired" });
  });

  it("keeps untimed scopes independent of the clock", () => {
    expect(trustStatus(contactWith({ messageTrust: "until_revoked" }), "message", now)).toEqual({ kind: "untilRevoked" });
    expect(trustStatus(contactWith({ messageTrust: "once" }), "message", now)).toEqual({ kind: "once" });
    expect(trustStatus(contactWith({}), "invite", now)).toEqual({ kind: "notAllowed" });
  });
});

describe("daysUntil", () => {
  it("rounds up partial days and never goes negative", () => {
    expect(daysUntil("2026-10-08T18:00:00.000Z", now)).toBe(1);
    expect(daysUntil("2026-10-15T12:00:00.000Z", now)).toBe(7);
    expect(daysUntil("2026-10-01T12:00:00.000Z", now)).toBe(0);
    expect(daysUntil("not a date", now)).toBe(0);
  });
});
