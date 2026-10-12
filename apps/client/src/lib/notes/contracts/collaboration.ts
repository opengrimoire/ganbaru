import type { NotesRichText } from "./core";

export interface NotesLocalUserUpdate {
  display_name: string;
}

export const NOTES_MENTION_NOTIFICATION_KINDS = [
  "reminder",
  "user_mention",
  "task_mention",
] as const;

export type NotesMentionNotificationKind =
  (typeof NOTES_MENTION_NOTIFICATION_KINDS)[number];

export const NOTES_MENTION_NOTIFICATION_TARGET_TYPES = [
  "date",
  "user",
  "project_task",
] as const;

export type NotesMentionNotificationTargetType =
  (typeof NOTES_MENTION_NOTIFICATION_TARGET_TYPES)[number];

export const NOTES_MENTION_NOTIFICATION_STATUSES = [
  "pending",
  "delivered",
  "dismissed",
] as const;

export type NotesMentionNotificationStatus =
  (typeof NOTES_MENTION_NOTIFICATION_STATUSES)[number];

export type NotesMentionNotificationSourceType = "block" | "comment";

export interface NotesMentionNotification {
  object: "mention_notification";
  id: string;
  source_type: NotesMentionNotificationSourceType;
  source_id: string;
  page_id: string;
  page_title: string;
  block_id: string | null;
  comment_id: string | null;
  kind: NotesMentionNotificationKind;
  target_type: NotesMentionNotificationTargetType;
  target_id: string | null;
  trigger_at: string | null;
  /** Minutes before `trigger_at` that a reminder fires; 0 for mentions. */
  remind_minutes_before: number;
  plain_text: string;
  source_plain_text: string;
  status: NotesMentionNotificationStatus;
  delivered_at: string | null;
  /** Silenced by the page notification level or a muted discussion, so it is never shown. */
  suppressed: boolean;
  created_time: string;
  last_edited_time: string;
}

export interface NotesMentionNotificationDeliveryUpdate {
  ids: string[];
}

export type NotesCommentParent =
  | { type: "page_id"; page_id: string }
  | { type: "block_id"; block_id: string };

export type NotesCommentThreadStatus = "open" | "resolved";

export interface NotesPartialUser {
  object: "user";
  id: string;
}

export interface NotesLocalUser {
  object: "user";
  id: string;
  display_name: string;
  created_time: string;
  last_edited_time: string;
}

export type NotesCommentDisplayName =
  | { type: "user"; resolved_name: string }
  | { type: "integration"; resolved_name: string }
  | { type: "custom"; resolved_name: string };

/** One emoji on a comment with everyone who chose it, in reaction order. */
export interface NotesCommentReaction {
  emoji: string;
  count: number;
  reacted_by_me: boolean;
  names: string[];
}

export interface NotesComment {
  object: "comment";
  id: string;
  parent: NotesCommentParent;
  discussion_id: string;
  created_time: string;
  last_edited_time: string;
  created_by: NotesPartialUser;
  rich_text: NotesRichText[];
  attachments: Record<string, unknown>[];
  display_name: NotesCommentDisplayName;
  deleted_at: string | null;
  reactions: NotesCommentReaction[];
}

export interface NotesCommentAnchor {
  object: "comment_anchor";
  type: "text_range";
  block_id: string;
  start: number;
  end: number;
  text: string;
  prefix: string;
  suffix: string;
  created_time: string;
  last_edited_time: string;
}

export interface NotesCommentThread {
  object: "comment_thread";
  id: string;
  parent: NotesCommentParent;
  page_id: string;
  block_id: string | null;
  status: NotesCommentThreadStatus;
  resolved_at: string | null;
  resolved_by: NotesPartialUser | null;
  anchor: NotesCommentAnchor | null;
  created_time: string;
  last_edited_time: string;
  unread: boolean;
  muted: boolean;
  comments: NotesComment[];
}

export interface NotesCommentAnchorCreate {
  start: number;
  end: number;
  text: string;
  prefix: string;
  suffix: string;
}

export interface NotesCommentCreate {
  id: string;
  parent?: NotesCommentParent;
  discussion_id?: string;
  anchor?: NotesCommentAnchorCreate;
  rich_text: NotesRichText[];
  attachments?: Record<string, unknown>[];
}

export interface NotesCommentUpdate {
  rich_text: NotesRichText[];
  attachments?: Record<string, unknown>[];
}

export interface NotesCommentThreadReadUpdate {
  page_id: string;
  discussion_ids: string[];
  include_resolved?: boolean;
}

export type NotesSuggestionStatus = "open" | "accepted" | "rejected";

export interface NotesSuggestion {
  object: "suggestion";
  id: string;
  page_id: string;
  block_id: string;
  created_by: NotesPartialUser;
  display_name: NotesCommentDisplayName;
  status: NotesSuggestionStatus;
  range_start: number;
  range_end: number;
  original_text: string;
  proposed_text: string;
  prefix: string;
  suffix: string;
  accepted_at: string | null;
  accepted_by: NotesPartialUser | null;
  rejected_at: string | null;
  rejected_by: NotesPartialUser | null;
  created_time: string;
  last_edited_time: string;
}

export interface NotesSuggestionCreate {
  id: string;
  block_id: string;
  range_start: number;
  range_end: number;
  original_text: string;
  proposed_text: string;
  prefix: string;
  suffix: string;
}

/** Which comment activity on one page notifies the local user. */
export const NOTES_PAGE_NOTIFICATION_LEVELS = ["all", "mentions", "none"] as const;

export type NotesPageNotificationLevel = (typeof NOTES_PAGE_NOTIFICATION_LEVELS)[number];

export interface NotesPageNotificationSettings {
  object: "page_notification_settings";
  page_id: string;
  level: NotesPageNotificationLevel;
}
