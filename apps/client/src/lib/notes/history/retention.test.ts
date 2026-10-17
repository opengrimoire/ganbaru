import { describe, expect, it } from "vitest";
import {
  DEFAULT_NOTES_HISTORY_RETENTION_DAYS,
  effectiveNotesHistoryRetentionDays,
  isNotesHistoryRetentionDays,
  isShorterNotesHistoryRetention,
  NOTES_HISTORY_RETENTION_OPTIONS,
} from "./retention";

describe("notes history retention", () => {
  it("keeps the supported bounded options and 30 day default", () => {
    expect(NOTES_HISTORY_RETENTION_OPTIONS).toEqual([0, 7, 30, 90, 180, 365]);
    expect(DEFAULT_NOTES_HISTORY_RETENTION_DAYS).toBe(30);
    expect(NOTES_HISTORY_RETENTION_OPTIONS.every(isNotesHistoryRetentionDays)).toBe(true);
    expect(isNotesHistoryRetentionDays(14)).toBe(false);
    expect(isNotesHistoryRetentionDays(null)).toBe(false);
  });

  it("uses the project value only when an override exists", () => {
    expect(effectiveNotesHistoryRetentionDays(30, 180)).toBe(180);
    expect(effectiveNotesHistoryRetentionDays(30, 0)).toBe(0);
    expect(effectiveNotesHistoryRetentionDays(90, null)).toBe(90);
    expect(effectiveNotesHistoryRetentionDays(7, undefined)).toBe(7);
  });

  it("detects only changes that shorten retained history", () => {
    expect(isShorterNotesHistoryRetention(7, 0)).toBe(true);
    expect(isShorterNotesHistoryRetention(90, 30)).toBe(true);
    expect(isShorterNotesHistoryRetention(30, 30)).toBe(false);
    expect(isShorterNotesHistoryRetention(30, 180)).toBe(false);
  });
});
