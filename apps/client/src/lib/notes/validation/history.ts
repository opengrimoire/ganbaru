import type { NotesPageHistorySettings, NotesPageHistorySnapshot } from "../contracts/history";
import { readTolerantNotesCover, readTolerantNotesIcon } from "./assets";
import { parseNotesPartialUser } from "./collaboration";
import { readDisplayString, readInteger, readRecord, readString } from "./readers";

export function parseNotesPageHistorySnapshot(value: unknown): NotesPageHistorySnapshot {
  const record = readRecord(value, "page history snapshot");
  if (record.object !== "page_history_snapshot") {
    throw new Error("page history snapshot.object must be page_history_snapshot");
  }
  const blockCount = readInteger(record.block_count, "page history snapshot.block_count");
  if (blockCount < 0) throw new Error("page history snapshot.block_count must not be negative");
  return {
    object: "page_history_snapshot",
    id: readString(record.id, "page history snapshot.id"),
    page_id: readString(record.page_id, "page history snapshot.page_id"),
    title: readString(record.title, "page history snapshot.title"),
    icon: readTolerantNotesIcon(record.icon, "page history snapshot.icon"),
    cover: readTolerantNotesCover(record.cover, "page history snapshot.cover"),
    block_count: blockCount,
    reason: readDisplayString(record.reason, "page history snapshot.reason"),
    created_by: parseNotesPartialUser(
      record.created_by,
      "page history snapshot.created_by",
    ),
    created_time: readString(record.created_time, "page history snapshot.created_time"),
    page_last_edited_time: readString(
      record.page_last_edited_time,
      "page history snapshot.page_last_edited_time",
    ),
  };
}

export function parseNotesPageHistorySettings(value: unknown): NotesPageHistorySettings {
  const record = readRecord(value, "page history settings");
  if (record.object !== "page_history_settings") {
    throw new Error("page history settings.object must be page_history_settings");
  }
  const retentionDays = readInteger(
    record.retention_days,
    "page history settings.retention_days",
  );
  if (![0, 7, 30, 90, 180, 365].includes(retentionDays)) {
    throw new Error("page history settings.retention_days is unsupported");
  }
  return {
    object: "page_history_settings",
    retention_days: retentionDays as 0 | 7 | 30 | 90 | 180 | 365,
    updated_at: readString(record.updated_at, "page history settings.updated_at"),
  };
}
