/**
 * Presentation rules for failures a person can see.
 *
 * Backend errors carry English diagnostic text meant for logs. Only text the frontend
 * produced from the message catalog may reach the screen, so a failure shows either
 * a {@link LocalizedError} message or a localized message the caller chose for the
 * failed action. Raw backend text stays available as secondary technical detail.
 */

/** Maximum length of technical detail kept for copying or secondary display. */
const MAX_TECHNICAL_DETAIL_CHARS = 2_000;

/** Upper bound on nested `cause` links followed while looking for localized text. */
const MAX_CAUSE_DEPTH = 4;

/** A failure whose message already comes from the message catalog and may be shown as is. */
export class LocalizedError extends Error {
  /**
   * @param message Text produced by the translator for the active locale.
   * @param options Standard error options, such as the underlying `cause`.
   */
  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "LocalizedError";
  }
}

/**
 * The catalog text of a {@link LocalizedError} found in `error` or its cause chain, or null.
 *
 * @param error Any thrown or rejected value.
 */
export function localizedErrorText(error: unknown): string | null {
  let current: unknown = error;
  for (let depth = 0; depth <= MAX_CAUSE_DEPTH; depth += 1) {
    if (current instanceof LocalizedError) return current.message.trim() || null;
    if (!(current instanceof Error)) return null;
    current = current.cause;
  }
  return null;
}

/**
 * The text a person sees for a failure: localized text the failure carries, otherwise `fallback`.
 *
 * @param error Any thrown or rejected value.
 * @param fallback Localized message describing the failed action.
 */
export function userErrorMessage(error: unknown, fallback: string): string {
  return localizedErrorText(error) ?? fallback;
}

/**
 * Raw diagnostic text of a failure, bounded, for logs, copy buttons, and secondary detail.
 * Never use it as the primary message.
 *
 * @param error Any thrown or rejected value.
 */
export function technicalErrorDetail(error: unknown): string | null {
  const text = rawErrorText(error);
  if (!text) return null;
  return text.length > MAX_TECHNICAL_DETAIL_CHARS ? `${text.slice(0, MAX_TECHNICAL_DETAIL_CHARS)}…` : text;
}

function rawErrorText(error: unknown): string | null {
  if (error instanceof Error) return trimmed(error.message);
  if (typeof error === "string") return trimmed(error);
  const record = objectRecord(error);
  if (!record) return null;
  const direct = trimmed(record.message) ?? trimmed(record.error) ?? trimmed(record.detail);
  if (direct) return direct;
  const nested = objectRecord(record.error);
  return trimmed(nested?.message) ?? trimmed(nested?.detail);
}

function objectRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function trimmed(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}
