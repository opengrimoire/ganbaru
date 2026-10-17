import { describe, expect, it } from "vitest";
import { notesDatabaseRowMatchesFilters } from "./filter-evaluation";
import type { NotesDatabaseTableColumn } from "./table";
import type { NotesDatabaseTableFilter, NotesDataSourcePropertyType, NotesPage } from "$lib/notes/types";

function column(id: string, type: NotesDataSourcePropertyType): NotesDatabaseTableColumn {
  return { id, type, name: id, hidden: false, width: 180, options: [], relationDataSourceId: null, buttonLabel: "", buttonRequiresConfirmation: false };
}
const columns = [column("title", "title"), column("text", "rich_text"), column("number", "number"), column("date", "date"), column("done", "checkbox"), column("created", "created_time"), column("computed", "formula")];
const WHITE_SPACE_CHARACTERS = [
  "\u0009", "\u000A", "\u000B", "\u000C", "\u000D", "\u0020", "\u0085", "\u00A0", "\u1680",
  "\u2000", "\u2001", "\u2002", "\u2003", "\u2004", "\u2005", "\u2006", "\u2007", "\u2008", "\u2009", "\u200A",
  "\u2028", "\u2029", "\u202F", "\u205F", "\u3000",
] as const;
const row: NotesPage = {
  object: "page", id: "row", created_time: "2026-10-01T23:00:00Z", last_edited_time: "2026-10-03T10:00:00Z",
  parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null, in_trash: false, icon: null, cover: null,
  properties: {
    Name: { id: "title", type: "title", title: [{ plain_text: "Plan work" }] },
    Amount: { id: "number", type: "number", number: 10 },
    Due: { id: "date", type: "date", date: { start: "2026-10-02T01:00:00+02:00", end: null, time_zone: null } },
    Done: { id: "done", type: "checkbox", checkbox: false },
    Created: { id: "created", type: "created_time", created_time: "1999-01-01T00:00:00Z" },
    Formula: { id: "computed", type: "formula", formula: { type: "number", number: 20 } },
  },
  url: null, public_url: null, source_provider: null, source_object_id: null, source_workspace_id: null, source_last_edited_time: null,
};

/** Supply one text payload while retaining unrelated populated properties. */
function rowWithText(text: string | null): NotesPage {
  return { ...row, properties: { ...row.properties, Text: { id: "text", type: "rich_text", rich_text: text === null ? null : [{ plain_text: text }] } } };
}

describe("Notes presentation filter evaluation", () => {
  it("evaluates nested Boolean groups and numbers numerically", () => {
    const filters: NotesDatabaseTableFilter[] = [{ type: "or", filters: [
      { property_id: "title", condition: "equals", value: "unrelated" },
      { type: "and", filters: [{ property_id: "number", condition: "greater_than", value: 2 }, { property_id: "done", condition: "unchecked" }] },
    ] }];
    expect(notesDatabaseRowMatchesFilters(row, columns, filters)).toBe(true);
    expect(notesDatabaseRowMatchesFilters(row, columns, [{ property_id: "number", condition: "less_than", value: 2 }])).toBe(false);
    expect(notesDatabaseRowMatchesFilters(row, columns, [{ property_id: "number", condition: "greater_than_or_equal", value: 10 }])).toBe(true);
    expect(notesDatabaseRowMatchesFilters(row, columns, [{ property_id: "title", condition: "contains", value: "PLAN" }])).toBe(true);
  });

  it.each([
    { text: "Plan WORK", value: "pLAN work", matches: true },
    { text: "CAFÉ", value: "cafÉ", matches: true },
    { text: "CAFÉ", value: "café", matches: false },
    { text: "café", value: "CAFÉ", matches: false },
    { text: "ÜBER", value: "Über", matches: true },
    { text: "ÜBER", value: "über", matches: false },
    { text: "東京 A", value: "東京 a", matches: true },
  ])("folds only ASCII letters when comparing $text with $value", ({ text, value, matches }) => {
    const candidate = rowWithText(text);
    expect(notesDatabaseRowMatchesFilters(candidate, columns, [{ property_id: "text", condition: "equals", value }])).toBe(matches);
    expect(notesDatabaseRowMatchesFilters(candidate, columns, [{ property_id: "text", condition: "not_equals", value }])).toBe(!matches);
    expect(notesDatabaseRowMatchesFilters(candidate, columns, [{ property_id: "text", condition: "contains", value }])).toBe(matches);
  });

  it("matches text fragments without folding accents or interpreting wildcard characters", () => {
    const candidate = rowWithText("A CAFÉ costs 10%_today");
    expect(notesDatabaseRowMatchesFilters(candidate, columns, [{ property_id: "text", condition: "contains", value: "cafÉ" }])).toBe(true);
    expect(notesDatabaseRowMatchesFilters(candidate, columns, [{ property_id: "text", condition: "contains", value: "café" }])).toBe(false);
    expect(notesDatabaseRowMatchesFilters(candidate, columns, [{ property_id: "text", condition: "contains", value: "%_" }])).toBe(true);
    expect(notesDatabaseRowMatchesFilters(candidate, columns, [{ property_id: "text", condition: "contains", value: "%costs%" }])).toBe(false);
  });

  it("treats exactly the 25 Unicode White_Space characters, null, and empty text as empty", () => {
    for (const text of [null, "", ...WHITE_SPACE_CHARACTERS, WHITE_SPACE_CHARACTERS.join("")]) {
      const candidate = rowWithText(text);
      expect(notesDatabaseRowMatchesFilters(candidate, columns, [{ property_id: "text", condition: "is_empty" }])).toBe(true);
      expect(notesDatabaseRowMatchesFilters(candidate, columns, [{ property_id: "text", condition: "is_not_empty" }])).toBe(false);
    }
  });

  it("keeps zero-width spaces, byte order marks, and non-whitespace controls populated", () => {
    for (const text of ["\u0000", "\u0001", "\u0008", "\u000E", "\u001F", "\u007F", "\u180E", "\u200B", "\u200C", "\uFEFF"]) {
      const candidate = rowWithText(`\u00A0${text}\u3000`);
      expect(notesDatabaseRowMatchesFilters(candidate, columns, [{ property_id: "text", condition: "is_empty" }])).toBe(false);
      expect(notesDatabaseRowMatchesFilters(candidate, columns, [{ property_id: "text", condition: "is_not_empty" }])).toBe(true);
    }
  });

  it("compares date-only values by UTC calendar day and timestamps by instant", () => {
    expect(notesDatabaseRowMatchesFilters(row, columns, [{ property_id: "date", condition: "equals", value: "2026-10-01" }])).toBe(true);
    expect(notesDatabaseRowMatchesFilters(row, columns, [{ property_id: "date", condition: "before", value: "2026-10-02" }])).toBe(true);
    expect(notesDatabaseRowMatchesFilters(row, columns, [{ property_id: "date", condition: "after", value: "2026-10-01T22:00:00Z" }])).toBe(true);
    expect(notesDatabaseRowMatchesFilters(row, columns, [{ property_id: "date", condition: "after", value: "2026-10-01T23:30:00Z" }])).toBe(false);
    expect(notesDatabaseRowMatchesFilters(row, columns, [{ property_id: "created", condition: "equals", value: "2026-10-01" }])).toBe(true);
  });

  it("preserves empty, zero, and false distinctions and excludes unavailable computed filters", () => {
    const empty = { ...row, properties: { Amount: { id: "number", type: "number", number: null } } };
    expect(notesDatabaseRowMatchesFilters(empty, columns, [{ property_id: "number", condition: "not_equals", value: 0 }])).toBe(false);
    expect(notesDatabaseRowMatchesFilters(empty, columns, [{ property_id: "number", condition: "is_empty" }])).toBe(true);
    expect(notesDatabaseRowMatchesFilters(empty, columns, [{ property_id: "done", condition: "unchecked" }])).toBe(false);
    expect(notesDatabaseRowMatchesFilters(row, columns, [{ property_id: "done", condition: "is_empty" }])).toBe(false);
    const zero = { ...row, properties: { Amount: { id: "number", type: "number", number: 0 } } };
    expect(notesDatabaseRowMatchesFilters(zero, columns, [{ property_id: "number", condition: "equals", value: 0 }])).toBe(true);
    expect(notesDatabaseRowMatchesFilters(row, columns, [{ property_id: "computed", condition: "greater_than", value: 10 }])).toBe(false);
    expect(notesDatabaseRowMatchesFilters(row, columns, [{ property_id: "computed", condition: "is_empty" }])).toBe(false);
    expect(notesDatabaseRowMatchesFilters(row, columns, [{ property_id: "computed", condition: "is_not_empty" }])).toBe(false);
    expect(notesDatabaseRowMatchesFilters({ ...row, properties: {} }, columns, [{ property_id: "computed", condition: "is_empty" }])).toBe(false);
    expect(notesDatabaseRowMatchesFilters(row, columns, [{ property_id: "missing", condition: "is_empty" }])).toBe(false);
  });
});
