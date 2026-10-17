import type { Contact, ContactsErrorCode } from "$lib/api/contacts";
import { formatRelativeDays } from "$lib/i18n/formatters";
import type { Translate } from "$lib/i18n/translator.svelte";
import { daysUntil, trustStatus, type ContactTrustScope, type ContactTrustStatus } from "./trust";

export type ContactsMutationErrorKey = "revisionConflict" | "readOnly" | "failed";

/** Which contacts error sentence explains a failed mutation. */
export function contactsMutationErrorKey(code: ContactsErrorCode): ContactsMutationErrorKey {
  switch (code) {
    case "revision_conflict":
      return "revisionConflict";
    case "read_only":
    case "identity_unavailable":
      return "readOnly";
    default:
      return "failed";
  }
}

/** Short lowercase phrase for one resolved trust scope, for use inside a row detail. */
export function formatTrustSummary(t: Translate, status: ContactTrustStatus, now: Date): string {
  switch (status.kind) {
    case "notAllowed":
      return t("contacts.list.trustSummary.notAllowed");
    case "once":
      return t("contacts.list.trustSummary.once");
    case "untilRevoked":
      return t("contacts.list.trustSummary.untilRevoked");
    case "until":
      return t("contacts.list.trustSummary.until", formatRelativeDays(t, daysUntil(status.expiresAt, now)));
    case "expired":
      return t("contacts.list.trustSummary.expired");
  }
}

/** Row detail describing both trust scopes of a contact as of now. */
export function formatContactTrustDetail(t: Translate, contact: Contact, now: Date): string {
  const summary = (scope: ContactTrustScope) => formatTrustSummary(t, trustStatus(contact, scope, now), now);
  return t("contacts.list.trustDetail", summary("invite"), summary("message"));
}
