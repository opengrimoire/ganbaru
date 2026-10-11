import { describe, expect, it } from "vitest";
import { parseNotesBlock, parseNotesCreatedDatabase, parseNotesDataSourceCsvExportSaveResult, parseNotesDataSourceCsvImportResult, parseNotesDataSourceListItem, parseNotesPage } from "./validation";
import { baseBlock, basePage, baseRichText } from "./validation.fixtures";

describe("notes database boundary validation", () => {
  it("parses data source row page parents", () => {
      const page = parseNotesPage({
        ...basePage,
        parent: {
          type: "data_source_id",
          data_source_id: "81818181-8181-4181-8181-818181818181",
        },
      });

      expect(page.parent).toEqual({
        type: "data_source_id",
        data_source_id: "81818181-8181-4181-8181-818181818181",
      });
    });

  it("parses child database payloads", () => {
      const block = parseNotesBlock({
        ...baseBlock,
        type: "child_database",
        child_database: {
          title: "Tasks",
          database_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
          data_source_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
          view_id: "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        },
      });

      expect(block.type).toBe("child_database");
      if (block.type === "child_database") {
        expect(block.child_database.title).toBe("Tasks");
        expect(block.child_database.database_id).toBe("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
        expect(block.child_database.data_source_id).toBe("cccccccc-cccc-4ccc-8ccc-cccccccccccc");
        expect(block.child_database.view_id).toBe("dddddddd-dddd-4ddd-8ddd-dddddddddddd");
      }
    });

  it("parses created local database responses", () => {
      const created = parseNotesCreatedDatabase({
        database: {
          object: "database",
          id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
          parent: baseBlock.parent,
          title: "Tasks",
          title_rich_text: [baseRichText],
          description: [],
          icon: null,
          cover: null,
          in_trash: false,
          is_inline: true,
          data_sources: [{ id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc", name: "Tasks" }],
          url: null,
          public_url: null,
          source_provider: null,
          source_object_id: null,
          source_workspace_id: null,
          source_last_edited_time: null,
          created_time: baseBlock.created_time,
          last_edited_time: baseBlock.last_edited_time,
        },
        data_source: {
          object: "data_source",
          id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
          parent: {
            type: "database_id",
            database_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
          },
          database_parent: baseBlock.parent,
          title: "Tasks",
          title_rich_text: [baseRichText],
          description: [],
          icon: null,
          properties: {
            Name: { id: "title", name: "Name", type: "title", title: {} },
          },
          in_trash: false,
          source_provider: null,
          source_object_id: null,
          source_workspace_id: null,
          source_last_edited_time: null,
          created_time: baseBlock.created_time,
          last_edited_time: baseBlock.last_edited_time,
        },
        view: {
          object: "view",
          id: "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
          parent: {
            type: "database_id",
            database_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
          },
          data_source_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
          name: "Table",
          type: "table",
          filter: null,
          sorts: [],
          configuration: {
            type: "table",
            table: {
              property_order: ["title"],
              hidden_property_ids: [],
            },
          },
          url: null,
          source_provider: null,
          source_object_id: null,
          source_workspace_id: null,
          source_last_edited_time: null,
          created_time: baseBlock.created_time,
          last_edited_time: baseBlock.last_edited_time,
        },
        block: {
          ...baseBlock,
          type: "child_database",
          child_database: {
            title: "Tasks",
            database_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            data_source_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
            view_id: "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
          },
        },
      });

      expect(created.database.data_sources[0]?.id).toBe("cccccccc-cccc-4ccc-8ccc-cccccccccccc");
      expect(created.view.type).toBe("table");
      expect(created.block.child_database.database_id).toBe("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
    });

  it("parses data source list items with the pages that hold their database", () => {
      const dataSource = {
        object: "data_source",
        id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        parent: { type: "database_id", database_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb" },
        database_parent: baseBlock.parent,
        title: "Tasks",
        title_rich_text: [baseRichText],
        description: [],
        icon: null,
        properties: {},
        in_trash: false,
        source_provider: null,
        source_object_id: null,
        source_workspace_id: null,
        source_last_edited_time: null,
        created_time: baseBlock.created_time,
        last_edited_time: baseBlock.last_edited_time,
      };

      expect(parseNotesDataSourceListItem({ ...dataSource, location: ["Productivity", ""] }).location).toEqual(["Productivity", ""]);
      expect(() => parseNotesDataSourceListItem(dataSource)).toThrow("data source.location must be an array");
      expect(() => parseNotesDataSourceListItem({ ...dataSource, location: ["Productivity", 3] })).toThrow("data source.location[1]");
    });

  it("rejects child database titles with control characters", () => {
      expect(() =>
        parseNotesBlock({
          ...baseBlock,
          type: "child_database",
          child_database: {
            title: "Tasks\u0008",
          },
        }),
      ).toThrow("block.child_database.title must not contain control characters");
    });

  it("parses data source CSV import result DTOs", () => {
      const result = parseNotesDataSourceCsvImportResult({
        object: "notes_data_source_csv_import",
        data_source_id: "81818181-8181-4181-8181-818181818181",
        dry_run: true,
        total_row_count: 2,
        valid_row_count: 1,
        skipped_row_count: 1,
        imported_row_count: 0,
        imported_page_ids: [],
        columns: [
          {
            source_index: 1,
            source_name: "Name",
            property_id: "title",
            property_name: "Name",
            property_type: "title",
            mapped: true,
            read_only: false,
            warning: null,
          },
          {
            source_index: 2,
            source_name: "Ticket",
            property_id: "ticket",
            property_name: "Ticket",
            property_type: "unique_id",
            mapped: true,
            read_only: true,
            warning: "CSV column maps to a read-only property and will be skipped.",
          },
        ],
        rows: [
          {
            row_number: 2,
            title: "Alpha",
            valid: true,
            mapped_cell_count: 1,
            error_count: 0,
          },
          {
            row_number: 3,
            title: "Broken",
            valid: false,
            mapped_cell_count: 1,
            error_count: 1,
          },
        ],
        diagnostics: [
          {
            code: "invalid_cell",
            severity: "error",
            row_number: 3,
            column_index: 3,
            column_name: "Estimate",
            property_id: "estimate",
            message: "number value must be a valid number",
          },
        ],
      });

      expect(result.object).toBe("notes_data_source_csv_import");
      expect(result.valid_row_count).toBe(1);
      expect(result.columns[1]?.read_only).toBe(true);
      expect(result.rows[1]?.valid).toBe(false);
      expect(result.diagnostics[0]?.property_id).toBe("estimate");
    });

  it("parses data source CSV export save result DTOs", () => {
      const result = parseNotesDataSourceCsvExportSaveResult({
        saved: true,
        export: {
          object: "notes_data_source_csv_export",
          data_source_id: "81818181-8181-4181-8181-818181818181",
          database_id: "82828282-8282-4282-8282-828282828282",
          view_id: "83838383-8383-4383-8383-838383838383",
          scope: "view",
          file_name: "tasks-table.csv",
          csv: "Name\nAlpha\n",
          exported_row_count: 1,
          exported_property_count: 1,
          diagnostics: [
            {
              code: "csv_export_plain_text_property",
              severity: "warning",
              property_id: "files",
              property_name: "Files",
              message: "Files is exported as plain text.",
            },
          ],
        },
      });

      expect(result.saved).toBe(true);
      expect(result.export?.scope).toBe("view");
      expect(result.export?.csv).toBe("Name\nAlpha\n");
      expect(result.export?.diagnostics[0]?.property_id).toBe("files");
    });
});
