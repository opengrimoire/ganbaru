import { describe, expect, it } from "vitest";
import { parseNotesAgentBridgeExportResult, parseNotesAgentBridgeExportSaveResult, parseNotesHtmlArchiveSaveResult, parseNotesHtmlImportResult, parseNotesJsonGraphExportResult, parseNotesJsonGraphExportSaveResult, parseNotesMarkdownExportResult, parseNotesNotionApiImportResult, parseNotesNotionExportImportResult } from "./validation";
import { baseBlock, basePage, baseRichText } from "./validation.fixtures";

describe("notes transfer boundary validation", () => {
  it("parses HTML import result DTOs", () => {
      const result = parseNotesHtmlImportResult({
        page: {
          page: { ...basePage, source_provider: "html", source_object_id: "import.html" },
          blocks: {
            object: "list",
            type: "block",
            block: {},
            results: [
              {
                ...baseBlock,
                type: "paragraph",
                paragraph: {
                  rich_text: [baseRichText],
                  color: "default",
                },
              },
            ],
            next_cursor: null,
            has_more: false,
          },
        },
        diagnostics: [
          {
            code: "html_markup_sanitized",
            severity: "info",
            line: null,
            message: "Unsafe or unsupported HTML markup was removed before import.",
          },
        ],
        imported_block_count: 1,
      });

      expect(result.page.page.source_provider).toBe("html");
      expect(result.diagnostics[0]?.code).toBe("html_markup_sanitized");
      expect(result.imported_block_count).toBe(1);
    });

  it("parses Notion API import result DTOs", () => {
      const result = parseNotesNotionApiImportResult({
        object: "notes_notion_api_import",
        imported_pages: [
          {
            page: { ...basePage, source_provider: "notion", source_object_id: "source-page" },
            blocks: {
              object: "list",
              type: "block",
              block: {},
              results: [
                {
                  ...baseBlock,
                  type: "paragraph",
                  paragraph: {
                    rich_text: [baseRichText],
                    color: "default",
                  },
                  source_provider: "notion",
                  source_object_id: "source-block",
                },
              ],
              next_cursor: null,
              has_more: false,
            },
          },
        ],
        imported_data_sources: [
          {
            object_type: "data_source",
            source_object_id: "source-data-source",
            local_id: "81818181-8181-4181-8181-818181818181",
            title: "Tasks",
          },
        ],
        imported_users: [
          {
            source_user_id: "notion-user",
            name: "Avo Cado",
            user_type: "person",
          },
        ],
        diagnostics: [
          {
            code: "temporary_notion_file_url",
            severity: "warning",
            source_object_id: "source-block",
            message: "File URL may expire.",
          },
        ],
        request_count: 6,
        retry_count: 1,
        rate_limit_count: 1,
        imported_page_count: 1,
        imported_block_count: 1,
        imported_data_source_count: 1,
        imported_comment_count: 2,
        imported_user_count: 1,
        imported_file_count: 1,
        unsupported_block_count: 0,
      });

      expect(result.object).toBe("notes_notion_api_import");
      expect(result.imported_pages[0]?.page.source_provider).toBe("notion");
      expect(result.imported_data_sources[0]?.local_id).toBe("81818181-8181-4181-8181-818181818181");
      expect(result.imported_users[0]?.name).toBe("Avo Cado");
      expect(result.diagnostics[0]?.code).toBe("temporary_notion_file_url");
      expect(result.rate_limit_count).toBe(1);
    });

  it("parses Notion export folder import result DTOs", () => {
      const result = parseNotesNotionExportImportResult({
        object: "notes_notion_export_import",
        imported_pages: [
          {
            page: {
              ...basePage,
              source_provider: "notion_export",
              source_object_id: "export-page",
            },
            blocks: {
              object: "list",
              type: "block",
              block: {},
              results: [],
              next_cursor: null,
              has_more: false,
            },
          },
        ],
        imported_data_sources: [
          {
            object_type: "data_source",
            source_object_id: "tasks.csv",
            local_id: "81818181-8181-4181-8181-818181818181",
            title: "Tasks",
          },
        ],
        diagnostics: [
          {
            code: "sitemap_skipped",
            severity: "info",
            source_path: "index.html",
            message: "Sitemap skipped.",
          },
        ],
        imported_page_count: 1,
        imported_block_count: 2,
        imported_data_source_count: 1,
        imported_file_count: 0,
        skipped_file_count: 1,
        unsupported_block_count: 0,
      });

      expect(result.object).toBe("notes_notion_export_import");
      expect(result.imported_pages[0]?.page.source_provider).toBe("notion_export");
      expect(result.imported_data_sources[0]?.title).toBe("Tasks");
      expect(result.diagnostics[0]?.source_path).toBe("index.html");
      expect(result.skipped_file_count).toBe(1);
    });

  it("parses markdown export result DTOs", () => {
      const result = parseNotesMarkdownExportResult({
        object: "notes_markdown_export",
        page_id: "11111111-1111-4111-8111-111111111111",
        markdown: "# Export\n",
        diagnostics: [
          {
            code: "markdown_export_unsupported_block",
            severity: "warning",
            block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            comment_id: null,
            message: "Unsupported block content was preserved",
          },
        ],
        exported_block_count: 2,
        exported_comment_count: 1,
      });

      expect(result.object).toBe("notes_markdown_export");
      expect(result.diagnostics[0]?.block_id).toBe("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
      expect(result.exported_comment_count).toBe(1);
    });

  it("parses HTML archive save result DTOs", () => {
      const result = parseNotesHtmlArchiveSaveResult({
        object: "notes_html_archive_save",
        saved: false,
        export: null,
      });

      expect(result.saved).toBe(false);
      expect(result.export).toBeNull();
    });

  it("parses JSON graph export result DTOs", () => {
      const result = parseNotesJsonGraphExportResult({
        object: "notes_json_graph_export",
        export_version: 1,
        schema_version: "notes-json-graph.v1",
        generated_at: "2026-07-03T12:00:00.000Z",
        file_name: "ganbaru-notes-json-graph.json",
        content_type: "application/json; charset=utf-8",
        json: "{\"object\":\"notes_json_graph\"}",
        byte_size: 29,
        counts: {
          tables: {
            notes_pages: 1,
          },
        },
        diagnostics: [
          {
            code: "json_graph_rebuildable_fts_omitted",
            severity: "warning",
            table_name: "notes_search_fts",
            row_id: null,
            message: "FTS rows omitted",
          },
        ],
        exported_page_count: 1,
        exported_block_count: 2,
        exported_comment_count: 3,
        exported_data_source_count: 4,
        exported_file_count: 5,
        exported_index_record_count: 6,
        exported_property_schema_count: 7,
        exported_table_count: 8,
        exported_record_count: 9,
        warning_count: 1,
      });

      expect(result.object).toBe("notes_json_graph_export");
      expect(result.counts["tables"]).toEqual({ notes_pages: 1 });
      expect(result.diagnostics[0]?.table_name).toBe("notes_search_fts");
      expect(result.exported_property_schema_count).toBe(7);
    });

  it("parses JSON graph export save result DTOs", () => {
      const result = parseNotesJsonGraphExportSaveResult({
        object: "notes_json_graph_export_save",
        saved: false,
        export: null,
      });

      expect(result.saved).toBe(false);
      expect(result.export).toBeNull();
    });

  it("parses agent bridge export result DTOs", () => {
      const result = parseNotesAgentBridgeExportResult({
        object: "notes_agent_bridge_export",
        export_version: 1,
        schema_version: "notes-agent-bridge.v1",
        file_name: "ganbaru-agent-bridge.md",
        content_type: "text/markdown; charset=utf-8",
        markdown: "# Bridge\n",
        byte_size: 9,
        diagnostics: [
          {
            code: "agent_bridge_project_not_exported",
            severity: "warning",
            source_type: "project",
            source_id: "project-a",
            message: "Requested project was not found.",
          },
        ],
        exported_page_count: 1,
        exported_project_count: 2,
        exported_task_count: 3,
        exported_database_view_count: 4,
        exported_backlink_count: 5,
        warning_count: 1,
      });

      expect(result.object).toBe("notes_agent_bridge_export");
      expect(result.diagnostics[0]?.source_type).toBe("project");
      expect(result.exported_task_count).toBe(3);
    });

  it("parses agent bridge export save result DTOs", () => {
      const result = parseNotesAgentBridgeExportSaveResult({
        object: "notes_agent_bridge_export_save",
        saved: false,
        export: null,
      });

      expect(result.saved).toBe(false);
      expect(result.export).toBeNull();
    });
});
