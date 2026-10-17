import type { NotesCommentAnchor, NotesCommentDisplayName, NotesCommentThreadStatus } from "./collaboration";
import type { NotesBlockType, NotesPage } from "./core";

export type NotesBacklinkReferenceType =
  | "child_page"
  | "page_mention"
  | "link"
  | "database_relation"
  | "comment_mention"
  | "comment_link";

export interface NotesBacklink {
  object: "backlink";
  id: string;
  source_page: NotesPage;
  source_block_id: string;
  source_block_type: NotesBlockType | "database_relation" | "comment";
  reference_type: NotesBacklinkReferenceType;
  snippet: string;
  created_time: string;
  last_edited_time: string;
}

export interface NotesPageAlias {
  object: "page_alias";
  id: string;
  page_id: string;
  alias: string;
  normalized_alias: string;
  created_time: string;
  last_edited_time: string;
}

export interface NotesPageAliasCreate {
  id: string;
  alias: string;
}

export type NotesUnresolvedLinkSourceType = "block" | "comment";

export interface NotesUnresolvedLink {
  object: "unresolved_link";
  id: string;
  source_type: NotesUnresolvedLinkSourceType;
  source_page_id: string;
  source_block_id: string | null;
  source_comment_id: string | null;
  raw_url: string;
  raw_target: string;
  normalized_target: string;
  link_text: string;
  snippet: string;
  created_time: string;
  last_edited_time: string;
}

export interface NotesUnresolvedLinkResolve {
  target_page_id: string;
}

export type NotesSearchResultType = "page" | "block" | "comment";

export interface NotesSearchResult {
  object: "search_result";
  id: string;
  type: NotesSearchResultType;
  page: NotesPage;
  block_id: string | null;
  block_type: NotesBlockType | null;
  comment_id: string | null;
  discussion_id: string | null;
  comment_status: NotesCommentThreadStatus | null;
  comment_author: NotesCommentDisplayName | null;
  comment_anchor: NotesCommentAnchor | null;
  snippet: string;
  last_edited_time: string;
}

export interface NotesSearchWindow {
  results: NotesSearchResult[];
  next_cursor: string | null;
}

export type NotesPageBreadcrumbStatus = "workspace" | "active" | "archived" | "trashed" | "missing";

export interface NotesPageBreadcrumbItem {
  id: string | null;
  title: string;
  current: boolean;
  status: NotesPageBreadcrumbStatus;
}

/** A task a page may mention, from a project in the page's project group. */
export interface NotesMentionScopeTask {
  id: string;
  project_id: string;
  title: string;
  completed: boolean;
}

/**
 * A Calendar event or recurring series a page may mention, from a project in the page's project group. Times keep
 * the stored encoding: UTC instants for timed events and dates for all-day ones.
 */
export interface NotesMentionScopeEvent {
  id: string;
  title: string;
  start_time: string;
  end_time: string;
  all_day: boolean;
  recurring: boolean;
  calendar_id: string;
  project_id: string;
}

/** The tasks and Calendar events a page may mention; both are empty for a page outside every project. */
export interface NotesMentionScope {
  tasks: NotesMentionScopeTask[];
  events: NotesMentionScopeEvent[];
}
