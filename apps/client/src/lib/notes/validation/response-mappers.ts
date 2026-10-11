import {
  parseNotesBacklink,
  parseNotesBlock,
  parseNotesCommentThread,
  parseNotesCreatedDatabase,
  parseNotesDataSource,
  parseNotesDataSourceListItem,
  parseNotesDatabaseView,
  parseNotesDataSourceBoardView,
  parseNotesDataSourceCalendarView,
  parseNotesDataSourceCsvExportSaveResult,
  parseNotesDataSourceCsvImportResult,
  parseNotesDataSourceGalleryView,
  parseNotesDataSourceListView,
  parseNotesDataSourceSchema,
  parseNotesDataSourceTableView,
  parseNotesDataSourceTemplate,
  parseNotesDataSourceTimelineView,
  parseNotesFolder,
  parseNotesHtmlArchiveSaveResult,
  parseNotesHtmlImportResult,
  parseNotesAgentBridgeExportSaveResult,
  parseNotesJsonGraphExportSaveResult,
  parseNotesLocalUser,
  parseNotesLoadedPage,
  parseNotesPageOpenResponse,
  parseNotesBlockFrontier,
  parseNotesBlockOutline,
  parseNotesNotionApiImportResult,
  parseNotesNotionExportImportResult,
  parseNotesMentionNotification,
  parseNotesPage,
  parseNotesPageAlias,
  parseNotesPageBreadcrumbItem,
  parseNotesPageHistorySettings,
  parseNotesPageHistorySnapshot,
  parseNotesPageTemplate,
  parseNotesPaginatedBlockList,
  parseNotesSearchResult,
  parseNotesSuggestion,
  parseNotesUnresolvedLink,
} from "$lib/notes/blocks/validation";
import type {
  NotesBacklink,
  NotesBlock,
  NotesCommentThread,
  NotesCreatedDatabase,
  NotesDataSource,
  NotesDataSourceListItem,
  NotesDatabaseView,
  NotesDataSourceBoardView,
  NotesDataSourceCalendarView,
  NotesDataSourceCsvExportSaveResult,
  NotesDataSourceCsvImportResult,
  NotesDataSourceGalleryView,
  NotesDataSourceListView,
  NotesDataSourceSchema,
  NotesDataSourceTableView,
  NotesDataSourceTemplate,
  NotesDataSourceTimelineView,
  NotesFolder,
  NotesHtmlArchiveSaveResult,
  NotesHtmlImportResult,
  NotesAgentBridgeExportSaveResult,
  NotesJsonGraphExportSaveResult,
  NotesLocalUser,
  NotesLoadedPage,
  NotesPageOpenResponse,
  NotesBlockFrontier,
  NotesBlockOutline,
  NotesNotionApiImportResult,
  NotesNotionExportImportResult,
  NotesMentionNotification,
  NotesPage,
  NotesPageAlias,
  NotesPageBreadcrumbItem,
  NotesPageHistorySettings,
  NotesPageHistorySnapshot,
  NotesPageTemplate,
  NotesPaginatedBlockList,
  NotesSearchResult,
  NotesSuggestion,
  NotesUnresolvedLink,
} from "$lib/notes/types";

/** Validate and map an unknown page DTO from the Tauri boundary. */
export function mapNotesPageDto(value: unknown): NotesPage {
  return parseNotesPage(value);
}

/** Validate and map an unknown folder DTO from the Tauri boundary. */
export function mapNotesFolderDto(value: unknown): NotesFolder {
  return parseNotesFolder(value);
}

/** Validate and map an unknown block DTO from the Tauri boundary. */
export function mapNotesBlockDto(value: unknown): NotesBlock {
  return parseNotesBlock(value);
}

/** Validate and map an unknown paginated block list from the Tauri boundary. */
export function mapNotesBlockListDto(value: unknown): NotesPaginatedBlockList {
  return parseNotesPaginatedBlockList(value);
}

/** Validate and map an unknown loaded page DTO from the Tauri boundary. */
export function mapNotesLoadedPageDto(value: unknown): NotesLoadedPage {
  return parseNotesLoadedPage(value);
}

/** Validate and map the critical page-open response from the Tauri boundary. */
export function mapNotesPageOpenResponseDto(value: unknown): NotesPageOpenResponse {
  return parseNotesPageOpenResponse(value);
}

/** Validate and map a batched block frontier from the Tauri boundary. */
export function mapNotesBlockFrontierDto(value: unknown): NotesBlockFrontier {
  return parseNotesBlockFrontier(value);
}

/** Validate and map a lightweight block outline from the Tauri boundary. */
export function mapNotesBlockOutlineDto(value: unknown): NotesBlockOutline {
  return parseNotesBlockOutline(value);
}

/** Validate and map an unknown HTML import DTO from the Tauri boundary. */
export function mapNotesHtmlImportDto(value: unknown): NotesHtmlImportResult {
  return parseNotesHtmlImportResult(value);
}

/** Validate and map an unknown Notion API import DTO from the Tauri boundary. */
export function mapNotesNotionApiImportDto(value: unknown): NotesNotionApiImportResult {
  return parseNotesNotionApiImportResult(value);
}

/** Validate and map an unknown Notion export folder import DTO from the Tauri boundary. */
export function mapNotesNotionExportImportDto(value: unknown): NotesNotionExportImportResult {
  return parseNotesNotionExportImportResult(value);
}

/** Validate and map an unknown HTML archive save DTO from the Tauri boundary. */
export function mapNotesHtmlArchiveSaveDto(value: unknown): NotesHtmlArchiveSaveResult {
  return parseNotesHtmlArchiveSaveResult(value);
}

/** Validate and map an unknown JSON graph export save DTO from the Tauri boundary. */
export function mapNotesJsonGraphExportSaveDto(value: unknown): NotesJsonGraphExportSaveResult {
  return parseNotesJsonGraphExportSaveResult(value);
}

/** Validate and map an unknown agent bridge export save DTO from the Tauri boundary. */
export function mapNotesAgentBridgeExportSaveDto(
  value: unknown,
): NotesAgentBridgeExportSaveResult {
  return parseNotesAgentBridgeExportSaveResult(value);
}

/** Validate and map an unknown page breadcrumb item DTO from the Tauri boundary. */
export function mapNotesPageBreadcrumbItemDto(value: unknown): NotesPageBreadcrumbItem {
  return parseNotesPageBreadcrumbItem(value);
}

/** Validate and map an unknown page template DTO from the Tauri boundary. */
export function mapNotesPageTemplateDto(value: unknown): NotesPageTemplate {
  return parseNotesPageTemplate(value);
}

/** Validate and map an unknown page history snapshot DTO from the Tauri boundary. */
export function mapNotesPageHistorySnapshotDto(value: unknown): NotesPageHistorySnapshot {
  return parseNotesPageHistorySnapshot(value);
}

/** Validate and map an unknown page history settings DTO from the Tauri boundary. */
export function mapNotesPageHistorySettingsDto(value: unknown): NotesPageHistorySettings {
  return parseNotesPageHistorySettings(value);
}

/** Validate and map an unknown backlink DTO from the Tauri boundary. */
export function mapNotesBacklinkDto(value: unknown): NotesBacklink {
  return parseNotesBacklink(value);
}

/** Validate and map an unknown page alias DTO from the Tauri boundary. */
export function mapNotesPageAliasDto(value: unknown): NotesPageAlias {
  return parseNotesPageAlias(value);
}

/** Validate and map an unknown unresolved link DTO from the Tauri boundary. */
export function mapNotesUnresolvedLinkDto(value: unknown): NotesUnresolvedLink {
  return parseNotesUnresolvedLink(value);
}

/** Validate and map an unknown Notes search result DTO from the Tauri boundary. */
export function mapNotesSearchResultDto(value: unknown): NotesSearchResult {
  return parseNotesSearchResult(value);
}

/** Validate and map an unknown local Notes user DTO from the Tauri boundary. */
export function mapNotesLocalUserDto(value: unknown): NotesLocalUser {
  return parseNotesLocalUser(value);
}

/** Validate and map an unknown mention notification DTO from the Tauri boundary. */
export function mapNotesMentionNotificationDto(value: unknown): NotesMentionNotification {
  return parseNotesMentionNotification(value);
}

/** Validate and map an unknown comment thread DTO from the Tauri boundary. */
export function mapNotesCommentThreadDto(value: unknown): NotesCommentThread {
  return parseNotesCommentThread(value);
}

/** Validate and map an unknown suggestion DTO from the Tauri boundary. */
export function mapNotesSuggestionDto(value: unknown): NotesSuggestion {
  return parseNotesSuggestion(value);
}

/** Validate and map an unknown local database create DTO from the Tauri boundary. */
export function mapNotesCreatedDatabaseDto(value: unknown): NotesCreatedDatabase {
  return parseNotesCreatedDatabase(value);
}

/** Validate and map an unknown local data source DTO from the Tauri boundary. */
export function mapNotesDataSourceDto(value: unknown): NotesDataSource {
  return parseNotesDataSource(value);
}

/** Validate and map an unknown vault-wide data source list item from the Tauri boundary. */
export function mapNotesDataSourceListItemDto(value: unknown): NotesDataSourceListItem {
  return parseNotesDataSourceListItem(value);
}

/** Validate a saved database view returned by the Tauri boundary. */
export function mapNotesDatabaseViewDto(value: unknown): NotesDatabaseView {
  return parseNotesDatabaseView(value);
}

/** Validate and map an unknown local data source schema DTO from the Tauri boundary. */
export function mapNotesDataSourceSchemaDto(value: unknown): NotesDataSourceSchema {
  return parseNotesDataSourceSchema(value);
}

/** Validate and map an unknown local data source table view DTO from the Tauri boundary. */
export function mapNotesDataSourceTableViewDto(value: unknown): NotesDataSourceTableView {
  return parseNotesDataSourceTableView(value);
}

/** Validate and map an unknown local data source CSV import DTO from the Tauri boundary. */
export function mapNotesDataSourceCsvImportDto(value: unknown): NotesDataSourceCsvImportResult {
  return parseNotesDataSourceCsvImportResult(value);
}

/** Validate and map an unknown local data source CSV export save DTO from the Tauri boundary. */
export function mapNotesDataSourceCsvExportSaveDto(
  value: unknown,
): NotesDataSourceCsvExportSaveResult {
  return parseNotesDataSourceCsvExportSaveResult(value);
}

/** Validate and map an unknown local data source template DTO from the Tauri boundary. */
export function mapNotesDataSourceTemplateDto(value: unknown): NotesDataSourceTemplate {
  return parseNotesDataSourceTemplate(value);
}

/** Validate and map an unknown local data source board view DTO from the Tauri boundary. */
export function mapNotesDataSourceBoardViewDto(value: unknown): NotesDataSourceBoardView {
  return parseNotesDataSourceBoardView(value);
}

/** Validate and map an unknown local data source gallery view DTO from the Tauri boundary. */
export function mapNotesDataSourceGalleryViewDto(value: unknown): NotesDataSourceGalleryView {
  return parseNotesDataSourceGalleryView(value);
}

/** Validate and map an unknown local data source list view DTO from the Tauri boundary. */
export function mapNotesDataSourceListViewDto(value: unknown): NotesDataSourceListView {
  return parseNotesDataSourceListView(value);
}

/** Validate and map an unknown local data source calendar view DTO from the Tauri boundary. */
export function mapNotesDataSourceCalendarViewDto(value: unknown): NotesDataSourceCalendarView {
  return parseNotesDataSourceCalendarView(value);
}

/** Validate and map an unknown local data source timeline view DTO from the Tauri boundary. */
export function mapNotesDataSourceTimelineViewDto(value: unknown): NotesDataSourceTimelineView {
  return parseNotesDataSourceTimelineView(value);
}
