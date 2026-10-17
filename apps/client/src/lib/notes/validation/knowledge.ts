import type { NotesBacklink, NotesBacklinkReferenceType, NotesMentionScope, NotesMentionScopeEvent, NotesMentionScopeTask, NotesPageAlias, NotesPageBreadcrumbItem, NotesPageBreadcrumbStatus, NotesSearchResult, NotesSearchResultType, NotesUnresolvedLink, NotesUnresolvedLinkSourceType } from "../contracts/knowledge";
import { isNotesBlockType } from "./blocks";
import { parseCommentThreadStatus, parseNotesCommentAnchor, parseNotesCommentDisplayName } from "./collaboration";
import { readBoolean, readDisplayString, readMentionObjectId, readNullableString, readRecord, readRecordArray, readString } from "./readers";
import { parseNotesPage } from "./workspace";

function parseBacklinkReferenceType(value: unknown): NotesBacklinkReferenceType {
  const referenceType = readString(value, "backlink.reference_type");
  if (
    referenceType === "child_page"
    || referenceType === "page_mention"
    || referenceType === "link"
    || referenceType === "comment_mention"
    || referenceType === "comment_link"
  ) {
    return referenceType;
  }
  if (referenceType === "database_relation") return referenceType;
  throw new Error(
    "backlink.reference_type must be child_page, page_mention, link, database_relation, comment_mention, or comment_link",
  );
}

function parsePageBreadcrumbStatus(value: unknown): NotesPageBreadcrumbStatus {
  const status = readString(value, "page breadcrumb.status");
  if (
    status === "workspace"
    || status === "active"
    || status === "archived"
    || status === "trashed"
    || status === "missing"
  ) {
    return status;
  }
  throw new Error("page breadcrumb.status must be workspace, active, archived, trashed, or missing");
}

function parseSearchResultType(value: unknown): NotesSearchResultType {
  const resultType = readString(value, "search_result.type");
  if (resultType === "page" || resultType === "block" || resultType === "comment") {
    return resultType;
  }
  throw new Error("search_result.type must be page, block, or comment");
}

function parseUnresolvedLinkSourceType(value: unknown): NotesUnresolvedLinkSourceType {
  const sourceType = readString(value, "unresolved_link.source_type");
  if (sourceType === "block" || sourceType === "comment") return sourceType;
  throw new Error("unresolved_link.source_type must be block or comment");
}

export function parseNotesPageBreadcrumbItem(value: unknown): NotesPageBreadcrumbItem {
  const record = readRecord(value, "page breadcrumb");
  const id = record.id === null ? null : readString(record.id, "page breadcrumb.id");
  return {
    id,
    title: readString(record.title, "page breadcrumb.title"),
    current: readBoolean(record.current, "page breadcrumb.current"),
    status: parsePageBreadcrumbStatus(record.status),
  };
}

export function parseNotesBacklink(value: unknown): NotesBacklink {
  const record = readRecord(value, "backlink");
  if (record.object !== "backlink") throw new Error("backlink.object must be backlink");
  const referenceType = parseBacklinkReferenceType(record.reference_type);
  const sourceBlockType = readString(record.source_block_type, "backlink.source_block_type");
  if (
    !isNotesBlockType(sourceBlockType)
    && !(referenceType === "database_relation" && sourceBlockType === "database_relation")
    && !(
      (referenceType === "comment_mention" || referenceType === "comment_link")
      && sourceBlockType === "comment"
    )
  ) {
    throw new Error("backlink.source_block_type must be a supported block type");
  }
  return {
    object: "backlink",
    id: readString(record.id, "backlink.id"),
    source_page: parseNotesPage(record.source_page),
    source_block_id: readString(record.source_block_id, "backlink.source_block_id"),
    source_block_type: sourceBlockType,
    reference_type: referenceType,
    snippet: readString(record.snippet, "backlink.snippet"),
    created_time: readString(record.created_time, "backlink.created_time"),
    last_edited_time: readString(record.last_edited_time, "backlink.last_edited_time"),
  };
}

export function parseNotesPageAlias(value: unknown): NotesPageAlias {
  const record = readRecord(value, "page alias");
  if (record.object !== "page_alias") throw new Error("page alias.object must be page_alias");
  return {
    object: "page_alias",
    id: readString(record.id, "page alias.id"),
    page_id: readString(record.page_id, "page alias.page_id"),
    alias: readDisplayString(record.alias, "page alias.alias"),
    normalized_alias: readDisplayString(record.normalized_alias, "page alias.normalized_alias"),
    created_time: readString(record.created_time, "page alias.created_time"),
    last_edited_time: readString(record.last_edited_time, "page alias.last_edited_time"),
  };
}

export function parseNotesUnresolvedLink(value: unknown): NotesUnresolvedLink {
  const record = readRecord(value, "unresolved_link");
  if (record.object !== "unresolved_link") {
    throw new Error("unresolved_link.object must be unresolved_link");
  }
  return {
    object: "unresolved_link",
    id: readString(record.id, "unresolved_link.id"),
    source_type: parseUnresolvedLinkSourceType(record.source_type),
    source_page_id: readString(record.source_page_id, "unresolved_link.source_page_id"),
    source_block_id: readNullableString(record.source_block_id, "unresolved_link.source_block_id"),
    source_comment_id: readNullableString(
      record.source_comment_id,
      "unresolved_link.source_comment_id",
    ),
    raw_url: readString(record.raw_url, "unresolved_link.raw_url"),
    raw_target: readDisplayString(record.raw_target, "unresolved_link.raw_target"),
    normalized_target: readDisplayString(
      record.normalized_target,
      "unresolved_link.normalized_target",
    ),
    link_text: readString(record.link_text, "unresolved_link.link_text"),
    snippet: readString(record.snippet, "unresolved_link.snippet"),
    created_time: readString(record.created_time, "unresolved_link.created_time"),
    last_edited_time: readString(record.last_edited_time, "unresolved_link.last_edited_time"),
  };
}

export function parseNotesSearchResult(value: unknown): NotesSearchResult {
  const record = readRecord(value, "search_result");
  if (record.object !== "search_result") {
    throw new Error("search_result.object must be search_result");
  }
  const resultType = parseSearchResultType(record.type);
  const blockType = record.block_type === null
    ? null
    : readString(record.block_type, "search_result.block_type");
  if (blockType !== null && !isNotesBlockType(blockType)) {
    throw new Error("search_result.block_type must be a supported block type");
  }
  return {
    object: "search_result",
    id: readString(record.id, "search_result.id"),
    type: resultType,
    page: parseNotesPage(record.page),
    block_id: readNullableString(record.block_id, "search_result.block_id"),
    block_type: blockType,
    comment_id: readNullableString(record.comment_id, "search_result.comment_id"),
    discussion_id: readNullableString(record.discussion_id, "search_result.discussion_id"),
    comment_status: record.comment_status === null
      ? null
      : parseCommentThreadStatus(record.comment_status),
    comment_author: record.comment_author === null
      ? null
      : parseNotesCommentDisplayName(record.comment_author, "search_result.comment_author"),
    comment_anchor: record.comment_anchor === null
      ? null
      : parseNotesCommentAnchor(record.comment_anchor),
    snippet: readString(record.snippet, "search_result.snippet"),
    last_edited_time: readString(record.last_edited_time, "search_result.last_edited_time"),
  };
}

function parseNotesMentionScopeTask(value: unknown, index: number): NotesMentionScopeTask {
  const label = `mention_scope.tasks[${index}]`;
  const record = readRecord(value, label);
  return {
    id: readMentionObjectId(record.id, `${label}.id`),
    project_id: readMentionObjectId(record.project_id, `${label}.project_id`),
    title: readString(record.title, `${label}.title`),
    completed: readBoolean(record.completed, `${label}.completed`),
  };
}

function parseNotesMentionScopeEvent(value: unknown, index: number): NotesMentionScopeEvent {
  const label = `mention_scope.events[${index}]`;
  const record = readRecord(value, label);
  return {
    id: readMentionObjectId(record.id, `${label}.id`),
    title: readString(record.title, `${label}.title`),
    start_time: readString(record.start_time, `${label}.start_time`),
    end_time: readString(record.end_time, `${label}.end_time`),
    all_day: readBoolean(record.all_day, `${label}.all_day`),
    recurring: readBoolean(record.recurring, `${label}.recurring`),
    calendar_id: readString(record.calendar_id, `${label}.calendar_id`),
    project_id: readMentionObjectId(record.project_id, `${label}.project_id`),
  };
}

/** Validate the tasks and Calendar events a page may mention. */
export function parseNotesMentionScope(value: unknown): NotesMentionScope {
  const record = readRecord(value, "mention_scope");
  return {
    tasks: readRecordArray(record.tasks, "mention_scope.tasks").map(parseNotesMentionScopeTask),
    events: readRecordArray(record.events, "mention_scope.events").map(parseNotesMentionScopeEvent),
  };
}
