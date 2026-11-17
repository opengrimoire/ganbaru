import { describe, expect, it } from "vitest";
import { en } from "$lib/i18n/messages/en";
import { es } from "$lib/i18n/messages/es";
import { LocalizedError } from "$lib/i18n/user-errors";
import { CHAT_ERROR_CODES, PROBE_STATES } from "./contracts";
import {
  CHAT_ERROR_REASON_MESSAGES,
  chatErrorCode,
  chatErrorDetail,
  chatErrorDetailText,
  chatErrorField,
  chatErrorMessage,
  chatErrorReason,
  providerProbeMessage,
} from "./error-presentation";

function catalogText(catalog: unknown, key: string): unknown {
  let current: unknown = catalog;
  for (const part of key.split(".")) {
    if (typeof current !== "object" || current === null || !Object.hasOwn(current, part)) return undefined;
    current = (current as Record<string, unknown>)[part];
  }
  return current;
}

describe("chatErrorMessage", () => {
  it("has a message for every Chat error code in every catalog", () => {
    for (const code of CHAT_ERROR_CODES) {
      expect(typeof catalogText(en, `chatErrors.codes.${code}`), code).toBe("string");
      expect(typeof catalogText(es, `chatErrors.codes.${code}`), code).toBe("string");
    }
  });

  it("has a message for every known reason in every catalog", () => {
    for (const [reason, key] of Object.entries(CHAT_ERROR_REASON_MESSAGES)) {
      expect(typeof catalogText(en, key), reason).toBe("string");
      expect(typeof catalogText(es, key), reason).toBe("string");
    }
  });

  it("shows the code message instead of the backend text", () => {
    expect(chatErrorMessage({
      code: "invalid_state_transition",
      message: "Chat provider session is not ready",
      recoverable: true,
    })).toBe(en.chatErrors.codes.invalid_state_transition);
    expect(chatErrorMessage({ error: { code: "executable_missing", message: "codex: not found" } }))
      .toBe(en.chatErrors.codes.executable_missing);
    expect(chatErrorMessage({ cause: { code: "busy" } })).toBe(en.chatErrors.codes.busy);
  });

  it("prefers a known reason over the code", () => {
    expect(chatErrorMessage({ code: "conflict", message: "Refused", details: { reason: "pending_replies" } }))
      .toBe(en.chat.organization.reviewPendingReplies);
    expect(chatErrorMessage({ code: "not_found", details: { reason: "executed_elsewhere" } }))
      .toBe(en.chatErrors.reasons.executedElsewhere);
  });

  it("ignores unknown reasons and falls through to the code", () => {
    expect(chatErrorMessage({ code: "conflict", details: { reason: "invented" } })).toBe(en.chatErrors.codes.conflict);
  });

  it("lets the caller's action message win over codes that only report a failure", () => {
    for (const code of ["validation", "protocol", "persistence", "internal"] as const) {
      expect(chatErrorMessage({ code, message: "raw" }, "Could not save")).toBe("Could not save");
      expect(chatErrorMessage({ code, message: "raw" })).toBe(en.chatErrors.codes[code]);
    }
    expect(chatErrorMessage({ code: "stale_revision" }, "Could not save")).toBe(en.chatErrors.codes.stale_revision);
  });

  it("never shows raw text from errors, strings, or unknown codes", () => {
    expect(chatErrorMessage(new Error("Provider process stopped"))).toBe(en.chatErrors.generic);
    expect(chatErrorMessage("database is locked", "Could not send message")).toBe("Could not send message");
    expect(chatErrorMessage({ code: "unknown", message: "Raw" }, "Could not send message")).toBe("Could not send message");
    expect(chatErrorMessage({ error: { message: "Provider process stopped" } })).toBe(en.chatErrors.generic);
  });

  it("shows localized frontend failures as they are", () => {
    expect(chatErrorMessage(new LocalizedError("Ya existe"), "Fallback")).toBe("Ya existe");
  });
});

describe("chatErrorDetail", () => {
  it("keeps the backend text for secondary display", () => {
    expect(chatErrorDetail({ code: "protocol", message: "unexpected frame" })).toBe("unexpected frame");
    expect(chatErrorDetail({ code: "protocol" })).toBeNull();
  });
});

describe("providerProbeMessage", () => {
  it("explains every unhealthy probe state", () => {
    for (const state of PROBE_STATES) {
      if (state === "healthy") continue;
      expect(providerProbeMessage(state)).toBe(en.chatErrors.codes[state]);
    }
  });

  it("falls back to the general unavailable message without a failed probe", () => {
    expect(providerProbeMessage("healthy")).toBe(en.chat.status.providerUnavailable);
    expect(providerProbeMessage(null)).toBe(en.chat.status.providerUnavailable);
  });
});

describe("chat error fields", () => {
  it("reads the field from direct and nested validation errors", () => {
    expect(chatErrorField({ field: "handle", message: "Already used" })).toBe("handle");
    expect(chatErrorField({ error: { field: "displayName", message: "Required" } })).toBe("displayName");
    expect(chatErrorField("Failed")).toBeNull();
  });

  it("reads only recognized direct and nested error codes", () => {
    expect(chatErrorCode({ code: "stale_revision" })).toBe("stale_revision");
    expect(chatErrorCode({ error: { code: "conflict" } })).toBe("conflict");
    expect(chatErrorCode({ cause: { code: "permission" } })).toBe("permission");
    expect(chatErrorCode({ code: "invented" })).toBeNull();
    expect(chatErrorCode("stale_revision")).toBeNull();
  });

  it("reads the detail reason from direct and nested errors and ignores malformed details", () => {
    expect(chatErrorReason({ code: "conflict", details: { reason: "pending_replies" } })).toBe("pending_replies");
    expect(chatErrorReason({ error: { details: { reason: "no_executing_device" } } })).toBe("no_executing_device");
    expect(chatErrorReason({ cause: { details: { reason: "input_starts_work" } } })).toBe("input_starts_work");
    expect(chatErrorReason({ code: "conflict", details: null })).toBeNull();
    expect(chatErrorReason({ details: { reason: 3 } })).toBeNull();
    expect(chatErrorReason({ details: ["pending_replies"] })).toBeNull();
    expect(chatErrorReason("pending_replies")).toBeNull();
  });

  it("reads other detail text such as the label a duplicate provider matches", () => {
    const error = { code: "validation", details: { reason: "duplicate_provider", existingLabel: " Claude work " } };
    expect(chatErrorDetailText(error, "existingLabel")).toBe("Claude work");
    expect(chatErrorDetailText({ error }, "existingLabel")).toBe("Claude work");
    expect(chatErrorDetailText({ details: { existingLabel: "" } }, "existingLabel")).toBeNull();
    expect(chatErrorDetailText(error, "missing")).toBeNull();
  });
});
