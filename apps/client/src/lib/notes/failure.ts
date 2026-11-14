import { technicalErrorDetail, userErrorMessage } from "$lib/i18n/user-errors";

/** A Notes failure ready to show: a localized message and optional technical detail for copying. */
export interface NotesFailure {
  /** Localized text shown to the person. */
  readonly message: string;
  /** Raw diagnostic text, only for a copy button or logs. */
  readonly detail: string | undefined;
}

/**
 * Describes a caught failure for display.
 *
 * @param error Any thrown or rejected value.
 * @param fallback Localized message for the failed action, used unless the error carries localized text.
 */
export function notesFailure(error: unknown, fallback: string): NotesFailure {
  return { message: userErrorMessage(error, fallback), detail: technicalErrorDetail(error) ?? undefined };
}

/** Detail recorded when a failure carries no text, so a stored failure stays present. */
const UNKNOWN_ERROR_DETAIL = "Unknown error";

/**
 * Raw diagnostic text of a failure that a store keeps for a view, never empty. Views show a localized message for
 * the failed action and offer this text only for copying.
 *
 * @param error Any thrown or rejected value.
 */
export function notesErrorDetail(error: unknown): string {
  return technicalErrorDetail(error) ?? UNKNOWN_ERROR_DETAIL;
}
