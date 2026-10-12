use crate::data_sources;
use crate::models::{
    NoteDataSourceCsvExportDiagnosticDto, NoteDataSourceCsvExportDto,
    NoteDataSourceCsvExportRequest, NoteDataSourceCsvExportSaveDto, NoteDatabaseViewRow,
    NotePageRow,
};
use crate::validation::rich_text_items_plain_text;
use serde_json::Value;
use sqlx::SqlitePool;
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const LOSSY_PROPERTY_TYPES: &[&str] =
    &["files", "people", "relation", "rollup", "formula", "button"];

pub async fn export_csv(
    pool: &SqlitePool,
    data_source_id: &str,
    request: NoteDataSourceCsvExportRequest,
) -> Result<NoteDataSourceCsvExportDto, String> {
    let scope = CsvExportScope::from_request(request.scope.as_deref())?;
    let database_id = request.database_id.as_deref();
    let view_id = request.view_id.as_deref();
    data_sources::views::validate_view_scope(data_source_id, database_id, view_id)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes CSV export: {e}"))?;
    let (data_source, database) = data_sources::views::load_active_data_source_and_database_tx(
        &mut tx,
        data_source_id,
        "table",
    )
    .await?;
    let view = data_sources::layouts::table::ensure_table_view_row_tx(
        &mut tx,
        &data_source,
        database_id,
        view_id,
    )
    .await?;
    let schema_properties = parse_json(&data_source.properties, "data source properties")?;
    let schema = data_sources::layouts::table::table_schema(&schema_properties)?;
    let filters = data_sources::views::stored_filters(
        view.filter.as_deref(),
        "database view filter",
        "table",
    )?;
    if scope == CsvExportScope::View {
        let property_types = schema
            .iter()
            .map(|property| (property.id.as_str(), property.property_type.as_str()))
            .collect();
        data_sources::views::canonical_filter(&filters, &property_types, "table")?;
    }
    let mut rows =
        data_sources::layouts::table::load_active_row_pages_tx(&mut tx, data_source_id).await?;
    rows = rows
        .into_iter()
        .map(|row| data_sources::layouts::table::normalized_row_for_schema(row, &schema))
        .collect::<Result<Vec<_>, _>>()?;
    data_sources::relations::hydrate_relation_titles_tx(&mut tx, &mut rows).await?;
    data_sources::rollups::hydrate_rollups_tx(
        &mut tx,
        data_source_id,
        &schema_properties,
        &mut rows,
    )
    .await?;
    data_sources::formulas::hydrate_formulas(&schema_properties, &mut rows)?;
    data_sources::buttons::hydrate_buttons(&schema_properties, &mut rows)?;

    let sorts = data_sources::views::stored_sorts(&view.sorts, "database view sorts", "table")?;
    if scope == CsvExportScope::View {
        rows.retain(|row| {
            data_sources::layouts::table::row_matches_filters(row, &schema, &filters)
        });
    }
    let sort_slice = if scope == CsvExportScope::View {
        sorts.as_slice()
    } else {
        &[]
    };
    data_sources::layouts::table::sort_rows(&mut rows, &schema, sort_slice);
    let columns = export_columns(&schema, &view, scope)?;
    let headers = stable_headers(&columns);
    let diagnostics = export_diagnostics(&columns);
    let csv = render_csv(&headers, &rows, &columns)?;
    let file_name = export_file_name(&data_source.title, &view.name, scope);
    tx.commit()
        .await
        .map_err(|e| format!("commit notes CSV export: {e}"))?;
    Ok(NoteDataSourceCsvExportDto {
        object: "notes_data_source_csv_export",
        data_source_id: data_source.id,
        database_id: database.id,
        view_id: view.id,
        scope: scope.as_str().to_string(),
        file_name,
        csv,
        exported_row_count: rows.len() as i64,
        exported_property_count: columns.len() as i64,
        diagnostics,
    })
}

pub fn write_csv(
    path: &Path,
    export: NoteDataSourceCsvExportDto,
) -> Result<NoteDataSourceCsvExportSaveDto, String> {
    require_csv_extension(path)?;
    write_csv_file(path, &export.csv)?;
    Ok(NoteDataSourceCsvExportSaveDto::saved(export))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CsvExportScope {
    View,
    All,
}

impl CsvExportScope {
    fn from_request(value: Option<&str>) -> Result<Self, String> {
        match value.unwrap_or("view").trim() {
            "" | "view" => Ok(Self::View),
            "all" => Ok(Self::All),
            _ => Err("CSV export scope must be view or all".to_string()),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::View => "view",
            Self::All => "all",
        }
    }
}

fn export_columns<'a>(
    schema: &'a [data_sources::layouts::table::TableProperty],
    view: &NoteDatabaseViewRow,
    scope: CsvExportScope,
) -> Result<Vec<&'a data_sources::layouts::table::TableProperty>, String> {
    let configuration = view
        .configuration
        .as_deref()
        .map(|value| parse_json(value, "table view configuration"))
        .transpose()?
        .unwrap_or(Value::Null);
    let table = configuration.get("table").and_then(Value::as_object);
    let order = table
        .and_then(|value| value.get("property_order"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let hidden = if scope == CsvExportScope::View {
        table
            .and_then(|value| value.get("hidden_property_ids"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default()
    } else {
        HashSet::new()
    };
    let by_id = schema
        .iter()
        .map(|property| (property.id.as_str(), property))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::new();
    let mut columns = Vec::new();
    for id in ["title"]
        .into_iter()
        .chain(order.iter().map(String::as_str))
    {
        if hidden.contains(id) && id != "title" {
            continue;
        }
        if let Some(property) = by_id.get(id) {
            if seen.insert(property.id.as_str()) {
                columns.push(*property);
            }
        }
    }
    let mut remaining = schema
        .iter()
        .filter(|property| !seen.contains(property.id.as_str()))
        .filter(|property| scope == CsvExportScope::All || !hidden.contains(&property.id))
        .collect::<Vec<_>>();
    remaining.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    columns.extend(remaining);
    Ok(columns)
}

fn stable_headers(columns: &[&data_sources::layouts::table::TableProperty]) -> Vec<String> {
    let mut counts = HashMap::<String, usize>::new();
    for property in columns {
        *counts.entry(property.name.to_lowercase()).or_default() += 1;
    }
    columns
        .iter()
        .map(|property| {
            let name = property.name.trim();
            let header = if name.is_empty() { &property.id } else { name };
            if counts
                .get(&property.name.to_lowercase())
                .copied()
                .unwrap_or(0)
                > 1
            {
                format!("{header} ({})", property.id)
            } else {
                header.to_string()
            }
        })
        .collect()
}

fn export_diagnostics(
    columns: &[&data_sources::layouts::table::TableProperty],
) -> Vec<NoteDataSourceCsvExportDiagnosticDto> {
    columns
        .iter()
        .filter(|property| LOSSY_PROPERTY_TYPES.contains(&property.property_type.as_str()))
        .map(|property| NoteDataSourceCsvExportDiagnosticDto {
            code: "csv_export_plain_text_property".to_string(),
            severity: "warning".to_string(),
            property_id: Some(property.id.clone()),
            property_name: Some(property.name.clone()),
            message: format!(
                "{} is exported as plain text, not as the full structured property value.",
                property.name
            ),
        })
        .collect()
}

fn render_csv(
    headers: &[String],
    rows: &[NotePageRow],
    columns: &[&data_sources::layouts::table::TableProperty],
) -> Result<String, String> {
    let mut csv = String::new();
    push_csv_record(&mut csv, headers);
    for row in rows {
        let properties = parse_json(&row.properties, "row page properties")?;
        let record = columns
            .iter()
            .map(|property| property_plain_text(row, &properties, property))
            .collect::<Vec<_>>();
        push_csv_record(&mut csv, &record);
    }
    Ok(csv)
}

fn push_csv_record(csv: &mut String, values: &[String]) {
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            csv.push(',');
        }
        csv.push_str(&escape_csv_cell(value));
    }
    csv.push('\n');
}

fn escape_csv_cell(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn property_plain_text(
    row: &NotePageRow,
    properties: &Value,
    property: &data_sources::layouts::table::TableProperty,
) -> String {
    let value = properties.get(&property.key);
    let payload = value.and_then(|value| value.get(&property.property_type));
    match property.property_type.as_str() {
        "title" | "rich_text" => payload
            .and_then(Value::as_array)
            .map(|items| rich_text_items_plain_text(items))
            .unwrap_or_default(),
        "number" => payload
            .and_then(Value::as_f64)
            .map(number_to_text)
            .unwrap_or_default(),
        "checkbox" => payload
            .and_then(Value::as_bool)
            .map(|value| value.to_string())
            .unwrap_or_default(),
        "select" | "status" | "place" => payload
            .and_then(|value| value.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        "relation" => data_sources::layouts::table::row_property_plain_text(row, property),
        "multi_select" | "files" | "people" => payload
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(named_value_text)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default(),
        "rollup" => payload
            .map(data_sources::rollups::rollup_plain_text)
            .unwrap_or_default(),
        "formula" => payload
            .map(data_sources::formulas::formula_plain_text)
            .unwrap_or_default(),
        "button" => payload
            .map(data_sources::buttons::button_plain_text)
            .unwrap_or_default(),
        "date" => payload.map(date_plain_text).unwrap_or_default(),
        "url" | "email" | "phone_number" => payload
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        "created_time" => row.created_time.clone(),
        "last_edited_time" => row.last_edited_time.clone(),
        "created_by" | "last_edited_by" => payload.and_then(named_value_text).unwrap_or_default(),
        "unique_id" => payload.map(unique_id_text).unwrap_or_default(),
        _ => String::new(),
    }
}

fn named_value_text(value: &Value) -> Option<String> {
    value
        .get("title")
        .and_then(Value::as_str)
        .or_else(|| value.get("name").and_then(Value::as_str))
        .or_else(|| value.get("url").and_then(Value::as_str))
        .or_else(|| value.get("id").and_then(Value::as_str))
        .map(str::to_string)
}

fn date_plain_text(value: &Value) -> String {
    let start = value
        .get("start")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let end = value.get("end").and_then(Value::as_str).unwrap_or_default();
    if !end.is_empty() && end != start {
        format!("{start} to {end}")
    } else {
        start.to_string()
    }
}

fn unique_id_text(value: &Value) -> String {
    let prefix = value
        .get("prefix")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let number = value
        .get("number")
        .and_then(Value::as_i64)
        .map(|value| value.to_string())
        .unwrap_or_default();
    format!("{prefix}{number}")
}

fn number_to_text(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn export_file_name(data_source_title: &str, view_name: &str, scope: CsvExportScope) -> String {
    let base = safe_file_stem(data_source_title).unwrap_or_else(|| "database".to_string());
    let suffix = match scope {
        CsvExportScope::View => safe_file_stem(view_name).unwrap_or_else(|| "view".to_string()),
        CsvExportScope::All => "full".to_string(),
    };
    format!("{base}-{suffix}.csv")
}

fn safe_file_stem(value: &str) -> Option<String> {
    let mut output = String::new();
    let mut last_was_separator = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if !last_was_separator {
            output.push('-');
            last_was_separator = true;
        }
    }
    let trimmed = output.trim_matches('-').to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn require_csv_extension(path: &Path) -> Result<(), String> {
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
    {
        Ok(())
    } else {
        Err("CSV export path must end in .csv".to_string())
    }
}

fn write_csv_file(path: &Path, csv: &str) -> Result<(), String> {
    let temporary_path = temporary_csv_path(path)?;
    let result = fs::File::create(&temporary_path)
        .map_err(|e| format!("create CSV export: {e}"))
        .and_then(|mut file| {
            file.write_all(csv.as_bytes())
                .map_err(|e| format!("write CSV export: {e}"))
        })
        .and_then(|()| {
            fs::rename(&temporary_path, path).map_err(|e| format!("save CSV export: {e}"))
        });
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

fn temporary_csv_path(path: &Path) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .ok_or_else(|| "CSV export path has no file name".to_string())?
        .to_string_lossy();
    Ok(path.with_file_name(format!("{file_name}.tmp")))
}

fn parse_json(value: &str, label: &str) -> Result<Value, String> {
    serde_json::from_str(value).map_err(|e| format!("parse {label}: {e}"))
}
