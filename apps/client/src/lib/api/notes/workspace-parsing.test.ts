import { describe, expect, it } from "vitest";
import { mapNavigationDatabase, mapPageSummary } from "./workspace-parsing";

function pageSummary(icon: string | null): Record<string, unknown> {
  return {
    id: "page-1",
    created_time: "2026-08-30T12:00:00Z",
    last_edited_time: "2026-08-30T12:00:00Z",
    parent_type: "workspace",
    parent_page_id: null,
    parent_block_id: null,
    parent_data_source_id: null,
    folder_id: null,
    title: "Page",
    project_id: null,
    icon,
  };
}

describe("Notes workspace parsing", () => {
  it("accepts empty database titles while validating every navigation identity", () => {
    const database = {
      id: "00000000-0000-4000-8000-000000000001", page_id: "00000000-0000-4000-8000-000000000002",
      data_source_id: "00000000-0000-4000-8000-000000000003", title: "",
    };
    expect(mapNavigationDatabase(database)).toEqual(database);
    for (const field of ["id", "page_id", "data_source_id"] as const) {
      expect(() => mapNavigationDatabase({ ...database, [field]: "invalid" })).toThrow("identities must be UUIDs");
    }
    expect(() => mapNavigationDatabase({ ...database, title: null })).toThrow("database.title must be a string");
  });
  it("reports stored icon JSON failures with field context", () => {
    expect(() => mapPageSummary(pageSummary("{{"))).toThrow(
      'page.icon for page "page-1" must contain valid JSON',
    );
  });
});
