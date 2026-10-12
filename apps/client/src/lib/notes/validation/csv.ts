import type { NotesDataSourceCsvExportDiagnostic, NotesDataSourceCsvExportDiagnosticSeverity, NotesDataSourceCsvExportResult, NotesDataSourceCsvExportSaveResult, NotesDataSourceCsvExportScope, NotesDataSourceCsvImportColumn, NotesDataSourceCsvImportDiagnostic, NotesDataSourceCsvImportDiagnosticSeverity, NotesDataSourceCsvImportResult, NotesDataSourceCsvImportRow } from "$lib/notes/contracts/database";
import { readBoolean, readInteger, readNonNegativeInteger, readNullableString, readRecord, readString, readUuidString } from "./readers";

export function parseNotesDataSourceCsvImportResult(
  value: unknown,
): NotesDataSourceCsvImportResult {
  const record = readRecord(value, "CSV import result");
  if (record.object !== "notes_data_source_csv_import") {
    throw new Error("CSV import result.object must be notes_data_source_csv_import");
  }
  if (!Array.isArray(record.imported_page_ids)) {
    throw new Error("CSV import result.imported_page_ids must be an array");
  }
  if (!Array.isArray(record.columns)) {
    throw new Error("CSV import result.columns must be an array");
  }
  if (!Array.isArray(record.rows)) {
    throw new Error("CSV import result.rows must be an array");
  }
  if (!Array.isArray(record.diagnostics)) {
    throw new Error("CSV import result.diagnostics must be an array");
  }
  return {
    object: "notes_data_source_csv_import",
    data_source_id: readUuidString(record.data_source_id, "CSV import result.data_source_id"),
    dry_run: readBoolean(record.dry_run, "CSV import result.dry_run"),
    total_row_count: readNonNegativeInteger(
      record.total_row_count,
      "CSV import result.total_row_count",
    ),
    valid_row_count: readNonNegativeInteger(
      record.valid_row_count,
      "CSV import result.valid_row_count",
    ),
    skipped_row_count: readNonNegativeInteger(
      record.skipped_row_count,
      "CSV import result.skipped_row_count",
    ),
    imported_row_count: readNonNegativeInteger(
      record.imported_row_count,
      "CSV import result.imported_row_count",
    ),
    imported_page_ids: record.imported_page_ids.map((pageId, index) =>
      readUuidString(pageId, `CSV import result.imported_page_ids[${index}]`)
    ),
    columns: record.columns.map(parseNotesDataSourceCsvImportColumn),
    rows: record.rows.map(parseNotesDataSourceCsvImportRow),
    diagnostics: record.diagnostics.map(parseNotesDataSourceCsvImportDiagnostic),
  };
}

function parseNotesDataSourceCsvImportColumn(
  value: unknown,
  index: number,
): NotesDataSourceCsvImportColumn {
  const record = readRecord(value, `CSV import result.columns[${index}]`);
  const sourceIndex = readInteger(
    record.source_index,
    `CSV import result.columns[${index}].source_index`,
  );
  if (sourceIndex < 1) {
    throw new Error(`CSV import result.columns[${index}].source_index must be positive`);
  }
  return {
    source_index: sourceIndex,
    source_name: readString(record.source_name, `CSV import result.columns[${index}].source_name`),
    property_id: readNullableString(
      record.property_id,
      `CSV import result.columns[${index}].property_id`,
    ),
    property_name: readNullableString(
      record.property_name,
      `CSV import result.columns[${index}].property_name`,
    ),
    property_type: readNullableString(
      record.property_type,
      `CSV import result.columns[${index}].property_type`,
    ),
    mapped: readBoolean(record.mapped, `CSV import result.columns[${index}].mapped`),
    read_only: readBoolean(record.read_only, `CSV import result.columns[${index}].read_only`),
    warning: readNullableString(record.warning, `CSV import result.columns[${index}].warning`),
  };
}

function parseNotesDataSourceCsvImportRow(
  value: unknown,
  index: number,
): NotesDataSourceCsvImportRow {
  const record = readRecord(value, `CSV import result.rows[${index}]`);
  const rowNumber = readInteger(record.row_number, `CSV import result.rows[${index}].row_number`);
  if (rowNumber < 1) {
    throw new Error(`CSV import result.rows[${index}].row_number must be positive`);
  }
  return {
    row_number: rowNumber,
    title: readString(record.title, `CSV import result.rows[${index}].title`),
    valid: readBoolean(record.valid, `CSV import result.rows[${index}].valid`),
    mapped_cell_count: readNonNegativeInteger(
      record.mapped_cell_count,
      `CSV import result.rows[${index}].mapped_cell_count`,
    ),
    error_count: readNonNegativeInteger(
      record.error_count,
      `CSV import result.rows[${index}].error_count`,
    ),
  };
}

function parseNotesDataSourceCsvImportDiagnostic(
  value: unknown,
  index: number,
): NotesDataSourceCsvImportDiagnostic {
  const record = readRecord(value, `CSV import result.diagnostics[${index}]`);
  const severity = readString(
    record.severity,
    `CSV import result.diagnostics[${index}].severity`,
  );
  if (!isDataSourceCsvImportDiagnosticSeverity(severity)) {
    throw new Error(`CSV import result.diagnostics[${index}].severity is unsupported`);
  }
  const rowNumber = record.row_number === null
    ? null
    : readInteger(record.row_number, `CSV import result.diagnostics[${index}].row_number`);
  const columnIndex = record.column_index === null
    ? null
    : readInteger(record.column_index, `CSV import result.diagnostics[${index}].column_index`);
  if (rowNumber !== null && rowNumber < 1) {
    throw new Error(`CSV import result.diagnostics[${index}].row_number must be positive`);
  }
  if (columnIndex !== null && columnIndex < 1) {
    throw new Error(`CSV import result.diagnostics[${index}].column_index must be positive`);
  }
  return {
    code: readString(record.code, `CSV import result.diagnostics[${index}].code`),
    severity,
    row_number: rowNumber,
    column_index: columnIndex,
    column_name: readNullableString(
      record.column_name,
      `CSV import result.diagnostics[${index}].column_name`,
    ),
    property_id: readNullableString(
      record.property_id,
      `CSV import result.diagnostics[${index}].property_id`,
    ),
    message: readString(record.message, `CSV import result.diagnostics[${index}].message`),
  };
}

function isDataSourceCsvImportDiagnosticSeverity(
  value: string,
): value is NotesDataSourceCsvImportDiagnosticSeverity {
  return value === "info" || value === "warning" || value === "error";
}

export function parseNotesDataSourceCsvExportResult(
  value: unknown,
): NotesDataSourceCsvExportResult {
  const record = readRecord(value, "CSV export result");
  if (record.object !== "notes_data_source_csv_export") {
    throw new Error("CSV export result.object must be notes_data_source_csv_export");
  }
  if (!Array.isArray(record.diagnostics)) {
    throw new Error("CSV export result.diagnostics must be an array");
  }
  const scope = readString(record.scope, "CSV export result.scope");
  if (!isDataSourceCsvExportScope(scope)) {
    throw new Error("CSV export result.scope is unsupported");
  }
  return {
    object: "notes_data_source_csv_export",
    data_source_id: readUuidString(record.data_source_id, "CSV export result.data_source_id"),
    database_id: readUuidString(record.database_id, "CSV export result.database_id"),
    view_id: readUuidString(record.view_id, "CSV export result.view_id"),
    scope,
    file_name: readString(record.file_name, "CSV export result.file_name"),
    csv: readString(record.csv, "CSV export result.csv"),
    exported_row_count: readNonNegativeInteger(
      record.exported_row_count,
      "CSV export result.exported_row_count",
    ),
    exported_property_count: readNonNegativeInteger(
      record.exported_property_count,
      "CSV export result.exported_property_count",
    ),
    diagnostics: record.diagnostics.map(parseNotesDataSourceCsvExportDiagnostic),
  };
}

export function parseNotesDataSourceCsvExportSaveResult(
  value: unknown,
): NotesDataSourceCsvExportSaveResult {
  const record = readRecord(value, "CSV export save result");
  const saved = readBoolean(record.saved, "CSV export save result.saved");
  const exportResult = record.export === null
    ? null
    : parseNotesDataSourceCsvExportResult(record.export);
  if (saved && exportResult === null) {
    throw new Error("CSV export save result.export is required when saved is true");
  }
  return {
    saved,
    export: exportResult,
  };
}

function parseNotesDataSourceCsvExportDiagnostic(
  value: unknown,
  index: number,
): NotesDataSourceCsvExportDiagnostic {
  const record = readRecord(value, `CSV export result.diagnostics[${index}]`);
  const severity = readString(
    record.severity,
    `CSV export result.diagnostics[${index}].severity`,
  );
  if (!isDataSourceCsvExportDiagnosticSeverity(severity)) {
    throw new Error(`CSV export result.diagnostics[${index}].severity is unsupported`);
  }
  return {
    code: readString(record.code, `CSV export result.diagnostics[${index}].code`),
    severity,
    property_id: readNullableString(
      record.property_id,
      `CSV export result.diagnostics[${index}].property_id`,
    ),
    property_name: readNullableString(
      record.property_name,
      `CSV export result.diagnostics[${index}].property_name`,
    ),
    message: readString(record.message, `CSV export result.diagnostics[${index}].message`),
  };
}

function isDataSourceCsvExportScope(value: string): value is NotesDataSourceCsvExportScope {
  return value === "view" || value === "all";
}

function isDataSourceCsvExportDiagnosticSeverity(
  value: string,
): value is NotesDataSourceCsvExportDiagnosticSeverity {
  return value === "info" || value === "warning" || value === "error";
}
