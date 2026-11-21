import { describe, expect, it } from "vitest";
import { MusicLibraryApiError } from "$lib/api/music-library";
import { en } from "$lib/i18n/messages/en";
import { es } from "$lib/i18n/messages/es";
import { LocalizedError } from "$lib/i18n/user-errors";
import { MUSIC_SOURCE_INPUT_ERROR_MESSAGE_KEYS } from "./sources";
import {
  localizedMusicError,
  MUSIC_LIBRARY_ERROR_MESSAGE_KEYS,
  musicErrorMessage,
  musicSourceInputErrorMessage,
  SOUNDSCAPE_ERROR_MESSAGE_KEYS,
  soundscapeErrorMessage,
} from "./user-errors";

function catalogText(catalog: unknown, key: string): unknown {
  return key.split(".").reduce<unknown>(
    (node, part) => (node && typeof node === "object" ? (node as Record<string, unknown>)[part] : undefined),
    catalog,
  );
}

const FALLBACK = "The action could not be finished, so try again.";

describe("music error catalogs", () => {
  it("has English and Spanish text for every mapped code", () => {
    const keys = [
      ...Object.values(MUSIC_LIBRARY_ERROR_MESSAGE_KEYS).filter((key) => key !== null),
      ...Object.values(MUSIC_SOURCE_INPUT_ERROR_MESSAGE_KEYS),
      ...Object.values(SOUNDSCAPE_ERROR_MESSAGE_KEYS),
    ];
    for (const key of keys) {
      expect(typeof catalogText(en, key), `en ${key}`).toBe("string");
      expect(typeof catalogText(es, key), `es ${key}`).toBe("string");
    }
  });
});

describe("musicErrorMessage", () => {
  it("keeps localized text the failure already carries", () => {
    expect(musicErrorMessage(new LocalizedError("Ya localizado."), FALLBACK)).toBe("Ya localizado.");
  });

  it("explains specific library codes", () => {
    expect(musicErrorMessage(new MusicLibraryApiError("stale-write", "row version 3"), FALLBACK)).toBe(en.music.errors.staleWrite);
    expect(musicErrorMessage(new MusicLibraryApiError("not-found", "missing row"), FALLBACK)).toBe(en.music.errors.notFound);
  });

  it("uses the action message for broad codes and raw text", () => {
    expect(musicErrorMessage(new MusicLibraryApiError("database", "disk I/O error"), FALLBACK)).toBe(FALLBACK);
    expect(musicErrorMessage(new Error("raw English detail"), FALLBACK)).toBe(FALLBACK);
    expect(musicErrorMessage({ code: "toString" }, FALLBACK)).toBe(FALLBACK);
  });

  it("wraps failures as localized errors that keep the original cause", () => {
    const original = new Error("raw English detail");
    const wrapped = localizedMusicError(original, "music.errors.libraryLoadFailed");
    expect(wrapped).toBeInstanceOf(LocalizedError);
    expect(wrapped.message).toBe(en.music.errors.libraryLoadFailed);
    expect(wrapped.cause).toBe(original);
  });
});

describe("musicSourceInputErrorMessage", () => {
  it("explains each source input error", () => {
    expect(musicSourceInputErrorMessage("not-absolute")).toBe(en.music.errors.sourceInputNotAbsolute);
  });
});

describe("soundscapeErrorMessage", () => {
  it("explains engine codes", () => {
    expect(soundscapeErrorMessage({ code: "audio-device", message: "no default output" })).toBe(en.music.errors.soundscapeAudioDevice);
    expect(soundscapeErrorMessage({ code: "local-file", message: "open failed" })).toBe(en.music.errors.soundscapeLocalFile);
  });

  it("explains library codes and falls back to the generic soundscape message", () => {
    expect(soundscapeErrorMessage(new MusicLibraryApiError("conflict", "row conflict"))).toBe(en.music.errors.conflict);
    expect(soundscapeErrorMessage(new Error("raw English detail"))).toBe(en.music.soundscape.genericError);
  });
});
