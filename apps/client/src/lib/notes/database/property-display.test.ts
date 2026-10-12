import { describe, expect, it } from "vitest";
import { getLocalization } from "$lib/i18n/translator.svelte";
import type { NotesDataSourcePropertyType, NotesPage } from "$lib/notes/types";
import { notesDatabaseTableCellEditValue, notesDatabaseTableEditValuesEqual, type NotesDatabaseTableColumn } from "./table";
import { notesDatabaseDateDisplay, notesDatabaseNumberDisplay, notesDatabasePropertyDisplayText, notesDatabasePropertyFiles } from "./property-display";

/** Provide canonical row metadata independently of the property payload under test. */
function row(type: NotesDataSourcePropertyType, payload: unknown): NotesPage {
  return {
    object: "page", id: "row", created_time: "2026-10-01T00:00:00Z", last_edited_time: "2026-10-02T00:00:00Z",
    parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null, in_trash: false,
    icon: null, cover: null, properties: { Value: { id: "value", type, [type]: payload } },
    url: null, public_url: null, source_provider: null, source_object_id: null,
    source_workspace_id: null, source_last_edited_time: null,
  };
}
function column(type: NotesDataSourcePropertyType): NotesDatabaseTableColumn {
  return { id: "value", name: "Value", type, hidden: false, width: 180, options: [], relationDataSourceId: null, buttonLabel: "", buttonRequiresConfirmation: false };
}
const context = { locale: "en" as const, t: getLocalization().t, now: new Date("2026-10-01T18:00:00Z") };

describe("Notes canonical property display", () => {
  it("uses persisted money, percent, and grouping formats while retaining raw numeric edits", () => {
    expect(notesDatabaseNumberDisplay("en", 0.125, "percent")).toBe("12.5%");
    expect(notesDatabaseNumberDisplay("en", 1234.5, "dollar")).toBe("$1,234.50");
    expect(notesDatabaseNumberDisplay("es", 0.125, "percent")).toContain("12,5");
    expect(notesDatabaseNumberDisplay("en", 1234.5, "number")).toBe("1234.5");
    expect(notesDatabaseNumberDisplay("en", 1234.5, "number_with_commas")).toBe("1,234.5");
    const property = { ...column("number"), numberFormat: "dollar" as const };
    expect(notesDatabasePropertyDisplayText(row("number", 1234.5), property, context)).toBe("$1,234.50");
    expect(notesDatabaseTableCellEditValue(row("number", 1234.5), property)).toBe("1234.5");
  });
  it("preserves date-only days and local wall times without applying the machine timezone", () => {
    expect(notesDatabaseDateDisplay({ start: "2026-10-01", end: "2026-10-03", time_zone: "America/Monterrey" }, context, { date_format: "iso" }))
      .toBe("2026-10-01 to 2026-10-03");
    expect(notesDatabaseDateDisplay({ start: "2026-10-01T09:30:00", end: null, time_zone: "America/Monterrey" }, context, { date_format: "iso", time_format: "24_hour" }))
      .toBe("2026-10-01, 09:30");
    expect(notesDatabaseDateDisplay({ start: "2026-10-01", end: null, time_zone: "America/Monterrey" }, context, { date_format: "relative" })).toBe("today");
  });
  it("applies named zones to timestamp display and can hide its time component", () => {
    const value = { start: "2026-10-01T02:30:00Z", end: null, time_zone: "America/Monterrey" };
    expect(notesDatabaseDateDisplay(value, context, { date_format: "iso", time_format: "hidden" })).toBe("2026-09-30");
    expect(notesDatabaseDateDisplay(value, context, { date_format: "iso", time_format: "24_hour" })).toBe("2026-09-30, 20:30");
  });
  it("reads complete date edit objects and compares range and zone changes independently", () => {
    const date = { start: "2026-10-01", end: "2026-10-03", time_zone: "America/Monterrey" };
    expect(notesDatabaseTableCellEditValue(row("date", date), column("date"))).toEqual(date);
    expect(notesDatabaseTableEditValuesEqual(date, { ...date })).toBe(true);
    expect(notesDatabaseTableEditValuesEqual(date, { ...date, end: null })).toBe(false);
    expect(notesDatabaseTableEditValuesEqual(date, { ...date, time_zone: "UTC" })).toBe(false);
    expect(notesDatabaseTableEditValuesEqual(date, date.start)).toBe(false);
  });
  it("shows file and people labels, supports metadata timestamps, and excludes unsafe file links", () => {
    const files = row("files", [{ name: "Brief.pdf", external: { url: "https://example.com/brief.pdf" } }, { name: "Unsafe", external: { url: "javascript:alert(1)" } }]);
    expect(notesDatabasePropertyDisplayText(files, column("files"), context)).toBe("Brief.pdf and Unsafe");
    expect(notesDatabasePropertyFiles(files, column("files"))[1].url).toBeNull();
    expect(notesDatabasePropertyDisplayText(row("people", [{ name: "Alex", id: "a" }, { name: "Sam", id: "b" }]), column("people"), context)).toBe("Alex and Sam");
    expect(notesDatabasePropertyDisplayText(row("created_by", { id: "a", name: "Alex" }), column("created_by"), context)).toBe("Alex");
    expect(notesDatabasePropertyDisplayText(row("created_time", null), column("created_time"), context, { date_format: "iso", time_format: "hidden" })).toBe("2026-10-01");
  });
});
