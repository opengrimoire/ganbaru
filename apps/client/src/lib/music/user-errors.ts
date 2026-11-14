import type { MusicLibraryApiErrorCode } from "$lib/api/music-library";
import { LocalizedError, localizedErrorText } from "$lib/i18n/user-errors";
import { translate, type TranslationKey } from "$lib/i18n/translator.svelte";
import { MUSIC_SOURCE_INPUT_ERROR_MESSAGE_KEYS, type MusicSourceInputError } from "./sources";

/** Catalog keys under `music.errors`. */
export type MusicErrorKey = Extract<TranslationKey, `music.errors.${string}`>;

/**
 * Specific messages for Music library error codes. Codes mapped to null are too broad to explain
 * on their own, so the caller's message for the failed action is shown instead.
 */
export const MUSIC_LIBRARY_ERROR_MESSAGE_KEYS: Readonly<Record<MusicLibraryApiErrorCode, MusicErrorKey | null>> = {
  validation: null,
  "not-found": "music.errors.notFound",
  conflict: "music.errors.conflict",
  "stale-write": "music.errors.staleWrite",
  database: null,
  unknown: null,
};

function isMusicLibraryErrorCode(value: unknown): value is MusicLibraryApiErrorCode {
  return typeof value === "string" && Object.hasOwn(MUSIC_LIBRARY_ERROR_MESSAGE_KEYS, value);
}

/** The Music library error code a failure carries, or null when it carries none. */
export function musicLibraryErrorCode(error: unknown): MusicLibraryApiErrorCode | null {
  if (typeof error !== "object" || error === null || !("code" in error)) return null;
  return isMusicLibraryErrorCode(error.code) ? error.code : null;
}

/**
 * The text a person sees for a failed Music action: localized text the failure carries, then a
 * specific message for its library error code, otherwise `fallback`.
 *
 * @param error Any thrown or rejected value.
 * @param fallback Localized message describing the failed action.
 */
export function musicErrorMessage(error: unknown, fallback: string): string {
  const localized = localizedErrorText(error);
  if (localized) return localized;
  const code = musicLibraryErrorCode(error);
  const key = code ? MUSIC_LIBRARY_ERROR_MESSAGE_KEYS[code] : null;
  return key ? translate(key) : fallback;
}

/**
 * Wraps a failure as a `LocalizedError` whose text a person can read, keeping the original as its
 * cause for logs.
 *
 * @param error Any thrown or rejected value.
 * @param fallbackKey Message describing the failed action, used when the failure carries no
 *   localized text or specific code.
 */
export function localizedMusicError(error: unknown, fallbackKey: MusicErrorKey): LocalizedError {
  if (error instanceof LocalizedError) return error;
  return new LocalizedError(musicErrorMessage(error, translate(fallbackKey)), { cause: error });
}

/** Error codes the native soundscape engine reports. */
export type SoundscapeErrorCode = "validation" | "audio-device" | "local-file" | "backend";

/** Message for every soundscape engine error code. */
export const SOUNDSCAPE_ERROR_MESSAGE_KEYS: Readonly<Record<SoundscapeErrorCode, TranslationKey>> = {
  validation: "music.errors.soundscapeInvalid",
  "audio-device": "music.errors.soundscapeAudioDevice",
  "local-file": "music.errors.soundscapeLocalFile",
  backend: "music.soundscape.genericError",
};

function isSoundscapeErrorCode(value: unknown): value is SoundscapeErrorCode {
  return typeof value === "string" && Object.hasOwn(SOUNDSCAPE_ERROR_MESSAGE_KEYS, value);
}

function soundscapeErrorCode(error: unknown): SoundscapeErrorCode | null {
  if (typeof error !== "object" || error === null || !("code" in error)) return null;
  return isSoundscapeErrorCode(error.code) ? error.code : null;
}

/**
 * The text a person sees for a failed soundscape action: localized text the failure carries,
 * then a message for its engine or library error code, otherwise the generic soundscape message.
 *
 * @param error Any thrown or rejected value.
 */
export function soundscapeErrorMessage(error: unknown): string {
  const localized = localizedErrorText(error);
  if (localized) return localized;
  const code = soundscapeErrorCode(error);
  if (code) return translate(SOUNDSCAPE_ERROR_MESSAGE_KEYS[code]);
  return musicErrorMessage(error, translate("music.soundscape.genericError"));
}

/**
 * The localized message for a source input error.
 *
 * @param error Why the typed source could not be parsed.
 */
export function musicSourceInputErrorMessage(error: MusicSourceInputError): string {
  return translate(MUSIC_SOURCE_INPUT_ERROR_MESSAGE_KEYS[error]);
}
