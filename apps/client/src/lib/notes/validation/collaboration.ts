import { NOTES_MENTION_NOTIFICATION_KINDS, NOTES_MENTION_NOTIFICATION_STATUSES, NOTES_MENTION_NOTIFICATION_TARGET_TYPES, NOTES_PAGE_NOTIFICATION_LEVELS } from "../contracts/collaboration";
import type { NotesComment, NotesCommentAnchor, NotesCommentReaction, NotesCommentDisplayName, NotesCommentParent, NotesCommentThread, NotesCommentThreadStatus, NotesLocalUser, NotesMentionNotification, NotesMentionNotificationKind, NotesMentionNotificationStatus, NotesMentionNotificationTargetType, NotesPageNotificationLevel, NotesPageNotificationSettings, NotesPartialUser, NotesSuggestion, NotesSuggestionStatus } from "../contracts/collaboration";
import { parseNotesParent } from "./blocks";
import { readBoolean, readDisplayString, readInteger, readNullableString, readRecord, readString, readStringArray } from "./readers";
import { parseNotesRichTextArray } from "./rich-text";

function parseNotesCommentParent(value: unknown, label: string): NotesCommentParent {
  const parent = parseNotesParent(value);
  if (parent.type === "page_id" || parent.type === "block_id") return parent;
  throw new Error(`${label}.type must be page_id or block_id`);
}

export function parseNotesPartialUser(value: unknown, label: string): NotesPartialUser {
  const record = readRecord(value, label);
  if (record.object !== "user") throw new Error(`${label}.object must be user`);
  return {
    object: "user" as const,
    id: readString(record.id, `${label}.id`),
  };
}

export function parseNotesLocalUser(value: unknown): NotesLocalUser {
  const record = readRecord(value, "local user");
  if (record.object !== "user") throw new Error("local user.object must be user");
  return {
    object: "user",
    id: readString(record.id, "local user.id"),
    display_name: readDisplayString(record.display_name, "local user.display_name"),
    created_time: readString(record.created_time, "local user.created_time"),
    last_edited_time: readString(record.last_edited_time, "local user.last_edited_time"),
  };
}

export function parseNotesCommentDisplayName(value: unknown, label: string): NotesCommentDisplayName {
  const record = readRecord(value, label);
  const type = readString(record.type, `${label}.type`);
  const resolvedName = readDisplayString(record.resolved_name, `${label}.resolved_name`);
  if (type === "user" || type === "integration" || type === "custom") {
    return { type, resolved_name: resolvedName };
  }
  throw new Error(`${label}.type must be user, integration, or custom`);
}

function parseCommentAttachments(value: unknown, label: string): Record<string, unknown>[] {
  if (!Array.isArray(value)) throw new Error(`${label} must be an array`);
  return value.map((attachment, index) => readRecord(attachment, `${label}[${index}]`));
}

function parseCommentReaction(value: unknown, label: string): NotesCommentReaction {
  const record = readRecord(value, label);
  const emoji = readString(record.emoji, `${label}.emoji`);
  if (!emoji.trim()) throw new Error(`${label}.emoji must not be empty`);
  const count = readInteger(record.count, `${label}.count`);
  const names = readStringArray(record.names, `${label}.names`);
  if (count < 1 || count !== names.length) {
    throw new Error(`${label}.count must match its names`);
  }
  return {
    emoji,
    count,
    reacted_by_me: readBoolean(record.reacted_by_me, `${label}.reacted_by_me`),
    names,
  };
}

function parseCommentReactions(value: unknown, label: string): NotesCommentReaction[] {
  if (!Array.isArray(value)) throw new Error(`${label} must be an array`);
  return value.map((reaction, index) => parseCommentReaction(reaction, `${label}[${index}]`));
}

/** Validate one page's notification level for the current local user. */
export function parseNotesPageNotificationSettings(value: unknown): NotesPageNotificationSettings {
  const record = readRecord(value, "page notification settings");
  if (record.object !== "page_notification_settings") {
    throw new Error("page notification settings.object must be page_notification_settings");
  }
  const level = readString(record.level, "page notification settings.level");
  if (!(NOTES_PAGE_NOTIFICATION_LEVELS as readonly string[]).includes(level)) {
    throw new Error("page notification settings.level is unsupported");
  }
  return {
    object: "page_notification_settings",
    page_id: readString(record.page_id, "page notification settings.page_id"),
    level: level as NotesPageNotificationLevel,
  };
}

function parseNotesMentionNotificationKind(value: unknown): NotesMentionNotificationKind {
  const kind = readString(value, "mention notification.kind");
  if ((NOTES_MENTION_NOTIFICATION_KINDS as readonly string[]).includes(kind)) {
    return kind as NotesMentionNotificationKind;
  }
  throw new Error("mention notification.kind is unsupported");
}

function parseNotesMentionNotificationTargetType(
  value: unknown,
): NotesMentionNotificationTargetType {
  const targetType = readString(value, "mention notification.target_type");
  if ((NOTES_MENTION_NOTIFICATION_TARGET_TYPES as readonly string[]).includes(targetType)) {
    return targetType as NotesMentionNotificationTargetType;
  }
  throw new Error("mention notification.target_type is unsupported");
}

function parseNotesMentionNotificationStatus(value: unknown): NotesMentionNotificationStatus {
  const status = readString(value, "mention notification.status");
  if ((NOTES_MENTION_NOTIFICATION_STATUSES as readonly string[]).includes(status)) {
    return status as NotesMentionNotificationStatus;
  }
  throw new Error("mention notification.status is unsupported");
}

export function parseNotesMentionNotification(value: unknown): NotesMentionNotification {
  const record = readRecord(value, "mention notification");
  if (record.object !== "mention_notification") {
    throw new Error("mention notification.object must be mention_notification");
  }
  const sourceType = readString(record.source_type, "mention notification.source_type");
  if (sourceType !== "block" && sourceType !== "comment") {
    throw new Error("mention notification.source_type is unsupported");
  }
  return {
    object: "mention_notification",
    id: readString(record.id, "mention notification.id"),
    source_type: sourceType,
    source_id: readString(record.source_id, "mention notification.source_id"),
    page_id: readString(record.page_id, "mention notification.page_id"),
    page_title: readString(record.page_title, "mention notification.page_title"),
    block_id: readNullableString(record.block_id, "mention notification.block_id"),
    comment_id: readNullableString(record.comment_id, "mention notification.comment_id"),
    kind: parseNotesMentionNotificationKind(record.kind),
    target_type: parseNotesMentionNotificationTargetType(record.target_type),
    target_id: readNullableString(record.target_id, "mention notification.target_id"),
    trigger_at: readNullableString(record.trigger_at, "mention notification.trigger_at"),
    plain_text: readString(record.plain_text, "mention notification.plain_text"),
    source_plain_text: readString(
      record.source_plain_text,
      "mention notification.source_plain_text",
    ),
    status: parseNotesMentionNotificationStatus(record.status),
    delivered_at: readNullableString(record.delivered_at, "mention notification.delivered_at"),
    suppressed: readBoolean(record.suppressed, "mention notification.suppressed"),
    created_time: readString(record.created_time, "mention notification.created_time"),
    last_edited_time: readString(
      record.last_edited_time,
      "mention notification.last_edited_time",
    ),
  };
}

export function parseCommentThreadStatus(value: unknown): NotesCommentThreadStatus {
  const status = readString(value, "comment thread.status");
  if (status === "open" || status === "resolved") return status;
  throw new Error("comment thread.status must be open or resolved");
}

function parseSuggestionStatus(value: unknown): NotesSuggestionStatus {
  const status = readString(value, "suggestion.status");
  if (status === "open" || status === "accepted" || status === "rejected") return status;
  throw new Error("suggestion.status must be open, accepted, or rejected");
}

export function parseNotesCommentAnchor(value: unknown): NotesCommentAnchor {
  const record = readRecord(value, "comment anchor");
  if (record.object !== "comment_anchor") {
    throw new Error("comment anchor.object must be comment_anchor");
  }
  if (record.type !== "text_range") {
    throw new Error("comment anchor.type must be text_range");
  }
  const start = readInteger(record.start, "comment anchor.start");
  const end = readInteger(record.end, "comment anchor.end");
  if (start < 0 || end <= start) {
    throw new Error("comment anchor range must be non-empty");
  }
  const text = readString(record.text, "comment anchor.text");
  if (!text.trim()) {
    throw new Error("comment anchor.text must not be empty");
  }
  return {
    object: "comment_anchor",
    type: "text_range",
    block_id: readString(record.block_id, "comment anchor.block_id"),
    start,
    end,
    text,
    prefix: readString(record.prefix, "comment anchor.prefix"),
    suffix: readString(record.suffix, "comment anchor.suffix"),
    created_time: readString(record.created_time, "comment anchor.created_time"),
    last_edited_time: readString(record.last_edited_time, "comment anchor.last_edited_time"),
  };
}

export function parseNotesComment(value: unknown): NotesComment {
  const record = readRecord(value, "comment");
  if (record.object !== "comment") throw new Error("comment.object must be comment");
  return {
    object: "comment",
    id: readString(record.id, "comment.id"),
    parent: parseNotesCommentParent(record.parent, "comment.parent"),
    discussion_id: readString(record.discussion_id, "comment.discussion_id"),
    created_time: readString(record.created_time, "comment.created_time"),
    last_edited_time: readString(record.last_edited_time, "comment.last_edited_time"),
    created_by: parseNotesPartialUser(record.created_by, "comment.created_by"),
    rich_text: parseNotesRichTextArray(record.rich_text, "comment.rich_text"),
    attachments: parseCommentAttachments(record.attachments, "comment.attachments"),
    display_name: parseNotesCommentDisplayName(record.display_name, "comment.display_name"),
    deleted_at: readNullableString(record.deleted_at, "comment.deleted_at"),
    reactions: parseCommentReactions(record.reactions, "comment.reactions"),
  };
}

export function parseNotesCommentThread(value: unknown): NotesCommentThread {
  const record = readRecord(value, "comment thread");
  if (record.object !== "comment_thread") {
    throw new Error("comment thread.object must be comment_thread");
  }
  if (!Array.isArray(record.comments)) {
    throw new Error("comment thread.comments must be an array");
  }
  return {
    object: "comment_thread",
    id: readString(record.id, "comment thread.id"),
    parent: parseNotesCommentParent(record.parent, "comment thread.parent"),
    page_id: readString(record.page_id, "comment thread.page_id"),
    block_id: readNullableString(record.block_id, "comment thread.block_id"),
    status: parseCommentThreadStatus(record.status),
    resolved_at: readNullableString(record.resolved_at, "comment thread.resolved_at"),
    resolved_by: record.resolved_by === null
      ? null
      : parseNotesPartialUser(record.resolved_by, "comment thread.resolved_by"),
    anchor: record.anchor === null
      ? null
      : parseNotesCommentAnchor(record.anchor),
    created_time: readString(record.created_time, "comment thread.created_time"),
    last_edited_time: readString(record.last_edited_time, "comment thread.last_edited_time"),
    unread: readBoolean(record.unread, "comment thread.unread"),
    muted: readBoolean(record.muted, "comment thread.muted"),
    comments: record.comments.map(parseNotesComment),
  };
}

export function parseNotesSuggestion(value: unknown): NotesSuggestion {
  const record = readRecord(value, "suggestion");
  if (record.object !== "suggestion") throw new Error("suggestion.object must be suggestion");
  const rangeStart = readInteger(record.range_start, "suggestion.range_start");
  const rangeEnd = readInteger(record.range_end, "suggestion.range_end");
  if (rangeStart < 0 || rangeEnd <= rangeStart) {
    throw new Error("suggestion range must be non-empty");
  }
  const originalText = readString(record.original_text, "suggestion.original_text");
  if (!originalText.trim()) throw new Error("suggestion.original_text must not be empty");
  return {
    object: "suggestion",
    id: readString(record.id, "suggestion.id"),
    page_id: readString(record.page_id, "suggestion.page_id"),
    block_id: readString(record.block_id, "suggestion.block_id"),
    created_by: parseNotesPartialUser(record.created_by, "suggestion.created_by"),
    display_name: parseNotesCommentDisplayName(record.display_name, "suggestion.display_name"),
    status: parseSuggestionStatus(record.status),
    range_start: rangeStart,
    range_end: rangeEnd,
    original_text: originalText,
    proposed_text: readString(record.proposed_text, "suggestion.proposed_text"),
    prefix: readString(record.prefix, "suggestion.prefix"),
    suffix: readString(record.suffix, "suggestion.suffix"),
    accepted_at: readNullableString(record.accepted_at, "suggestion.accepted_at"),
    accepted_by: record.accepted_by === null
      ? null
      : parseNotesPartialUser(record.accepted_by, "suggestion.accepted_by"),
    rejected_at: readNullableString(record.rejected_at, "suggestion.rejected_at"),
    rejected_by: record.rejected_by === null
      ? null
      : parseNotesPartialUser(record.rejected_by, "suggestion.rejected_by"),
    created_time: readString(record.created_time, "suggestion.created_time"),
    last_edited_time: readString(record.last_edited_time, "suggestion.last_edited_time"),
  };
}
