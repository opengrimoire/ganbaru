import type { NotesParent } from "./core";
import type { NotesLoadedPage } from "./workspace";

export type NotesMarkdownImportDiagnosticSeverity = "info" | "warning" | "error";

export interface NotesMarkdownImportDiagnostic {
  code: string;
  severity: NotesMarkdownImportDiagnosticSeverity;
  line: number | null;
  message: string;
}

export interface NotesMarkdownImportResult {
  page: NotesLoadedPage;
  diagnostics: NotesMarkdownImportDiagnostic[];
  imported_block_count: number;
}

export interface NotesHtmlImportRequest {
  parent: NotesParent;
  html: string;
  title?: string | null;
  source_name?: string | null;
  after_block_id?: string | null;
  keep_external_file_references?: boolean | null;
  project_id?: string | null;
}

export type NotesHtmlImportDiagnosticSeverity = "info" | "warning" | "error";

export interface NotesHtmlImportDiagnostic {
  code: string;
  severity: NotesHtmlImportDiagnosticSeverity;
  line: number | null;
  message: string;
}

export interface NotesHtmlImportResult {
  page: NotesLoadedPage;
  diagnostics: NotesHtmlImportDiagnostic[];
  imported_block_count: number;
}

export interface NotesNotionApiImportRequest {
  parent: NotesParent;
  integration_token: string;
  source_workspace_id?: string | null;
  page_ids?: string[];
  data_source_ids?: string[];
  include_comments?: boolean | null;
  include_users?: boolean | null;
  keep_external_file_references?: boolean | null;
  page_size?: number | null;
  project_id?: string | null;
}

export type NotesNotionApiImportDiagnosticSeverity = "info" | "warning" | "error";

export interface NotesNotionApiImportDiagnostic {
  code: string;
  severity: NotesNotionApiImportDiagnosticSeverity;
  source_object_id: string | null;
  message: string;
}

export interface NotesNotionApiImportedObject {
  object_type: string;
  source_object_id: string;
  local_id: string;
  title: string;
}

export interface NotesNotionApiImportedUser {
  source_user_id: string;
  name: string;
  user_type: string;
}

export interface NotesNotionApiImportResult {
  object: "notes_notion_api_import";
  imported_pages: NotesLoadedPage[];
  imported_data_sources: NotesNotionApiImportedObject[];
  imported_users: NotesNotionApiImportedUser[];
  diagnostics: NotesNotionApiImportDiagnostic[];
  request_count: number;
  retry_count: number;
  rate_limit_count: number;
  imported_page_count: number;
  imported_block_count: number;
  imported_data_source_count: number;
  imported_comment_count: number;
  imported_user_count: number;
  imported_file_count: number;
  unsupported_block_count: number;
}

export interface NotesNotionExportImportRequest {
  parent: NotesParent;
  export_root_path: string;
  source_workspace_id?: string | null;
  keep_external_file_references?: boolean | null;
  copy_local_file_references?: boolean | null;
  import_markdown?: boolean | null;
  import_html?: boolean | null;
  import_csv?: boolean | null;
  project_id?: string | null;
}

export type NotesNotionExportImportDiagnosticSeverity = "info" | "warning" | "error";

export interface NotesNotionExportImportDiagnostic {
  code: string;
  severity: NotesNotionExportImportDiagnosticSeverity;
  source_path: string | null;
  message: string;
}

export interface NotesNotionExportImportResult {
  object: "notes_notion_export_import";
  imported_pages: NotesLoadedPage[];
  imported_data_sources: NotesNotionApiImportedObject[];
  diagnostics: NotesNotionExportImportDiagnostic[];
  imported_page_count: number;
  imported_block_count: number;
  imported_data_source_count: number;
  imported_file_count: number;
  skipped_file_count: number;
  unsupported_block_count: number;
}

export type NotesMarkdownExportDiagnosticSeverity = "info" | "warning" | "error";

export interface NotesMarkdownExportDiagnostic {
  code: string;
  severity: NotesMarkdownExportDiagnosticSeverity;
  block_id: string | null;
  comment_id: string | null;
  message: string;
}

export interface NotesMarkdownExportResult {
  object: "notes_markdown_export";
  page_id: string;
  markdown: string;
  diagnostics: NotesMarkdownExportDiagnostic[];
  exported_block_count: number;
  exported_comment_count: number;
}

export interface NotesHtmlExportRequest {
  page_id: string;
  include_page_tree?: boolean | null;
  include_comments?: boolean | null;
  include_resolved_comments?: boolean | null;
  include_assets?: boolean | null;
  include_database_views?: boolean | null;
}

export type NotesHtmlExportDiagnosticSeverity = "info" | "warning" | "error";

export interface NotesHtmlExportDiagnostic {
  code: string;
  severity: NotesHtmlExportDiagnosticSeverity;
  page_id: string | null;
  block_id: string | null;
  asset_id: string | null;
  comment_id: string | null;
  message: string;
}

export interface NotesHtmlExportFile {
  path: string;
  content_type: string;
  contents: string;
  byte_size: number;
}

export interface NotesHtmlExportAsset {
  id: string;
  archive_path: string;
  source_path: string;
  content_type: string;
  byte_size: number;
  sha256: string;
  storage_state: string;
  exported: boolean;
}

export interface NotesHtmlExportResult {
  object: "notes_html_archive_export";
  root_page_id: string;
  files: NotesHtmlExportFile[];
  assets: NotesHtmlExportAsset[];
  diagnostics: NotesHtmlExportDiagnostic[];
  manifest_json: string;
  exported_page_count: number;
  exported_block_count: number;
  exported_asset_count: number;
  exported_comment_count: number;
  exported_database_view_count: number;
}

export interface NotesHtmlArchiveSaveResult {
  object: "notes_html_archive_save";
  saved: boolean;
  export: NotesHtmlExportResult | null;
}

export interface NotesJsonGraphExportRequest {
  include_indexes?: boolean | null;
  include_history?: boolean | null;
  include_templates?: boolean | null;
  include_local_state?: boolean | null;
  pretty?: boolean | null;
}

export type NotesJsonGraphExportDiagnosticSeverity = "info" | "warning" | "error";

export interface NotesJsonGraphExportDiagnostic {
  code: string;
  severity: NotesJsonGraphExportDiagnosticSeverity;
  table_name: string | null;
  row_id: string | null;
  message: string;
}

export interface NotesJsonGraphExportResult {
  object: "notes_json_graph_export";
  export_version: number;
  schema_version: string;
  generated_at: string;
  file_name: string;
  content_type: string;
  json: string;
  byte_size: number;
  counts: Record<string, unknown>;
  diagnostics: NotesJsonGraphExportDiagnostic[];
  exported_page_count: number;
  exported_block_count: number;
  exported_comment_count: number;
  exported_data_source_count: number;
  exported_file_count: number;
  exported_index_record_count: number;
  exported_property_schema_count: number;
  exported_table_count: number;
  exported_record_count: number;
  warning_count: number;
}

export interface NotesJsonGraphExportSaveResult {
  object: "notes_json_graph_export_save";
  saved: boolean;
  export: NotesJsonGraphExportResult | null;
}

export interface NotesAgentBridgeExportRequest {
  page_ids?: string[];
  project_ids?: string[];
  include_descendants?: boolean | null;
  include_backlinks?: boolean | null;
  include_database_views?: boolean | null;
  include_task_context?: boolean | null;
  include_page_comments?: boolean | null;
  include_resolved_comments?: boolean | null;
}

export type NotesAgentBridgeExportDiagnosticSeverity = "info" | "warning" | "error";

export interface NotesAgentBridgeExportDiagnostic {
  code: string;
  severity: NotesAgentBridgeExportDiagnosticSeverity;
  source_type: string | null;
  source_id: string | null;
  message: string;
}

export interface NotesAgentBridgeExportResult {
  object: "notes_agent_bridge_export";
  export_version: number;
  schema_version: string;
  file_name: string;
  content_type: string;
  markdown: string;
  byte_size: number;
  diagnostics: NotesAgentBridgeExportDiagnostic[];
  exported_page_count: number;
  exported_project_count: number;
  exported_task_count: number;
  exported_database_view_count: number;
  exported_backlink_count: number;
  warning_count: number;
}

export interface NotesAgentBridgeExportSaveResult {
  object: "notes_agent_bridge_export_save";
  saved: boolean;
  export: NotesAgentBridgeExportResult | null;
}
