use crate::models::{
    NoteDataSourceCsvImportColumnDto, NoteDataSourceCsvImportDiagnosticDto,
    NoteDataSourceCsvImportDto, NoteDataSourceCsvImportRequest, NoteDataSourceCsvImportRowDto,
    NoteDataSourceRow, NoteDatabaseRow,
};
use crate::validation::{plain_text_from_payload, require_uuid};
use crate::{assets, data_sources, writes};
use serde_json::{Map, Value, json};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::collections::{HashMap, HashSet};

const MAX_CSV_BYTES: usize = 512 * 1024;
const MAX_CSV_ROWS: usize = 1_000;
const MAX_CSV_COLUMNS: usize = 100;
const MAX_CELL_CHARS: usize = 2_000;
const READ_ONLY_PROPERTY_TYPES: &[&str] = &[
    "files",
    "people",
    "created_time",
    "created_by",
    "last_edited_time",
    "last_edited_by",
    "unique_id",
    "rollup",
    "formula",
    "button",
];

pub async fn import_csv(
    pool: &SqlitePool,
    data_source_id: &str,
    request: NoteDataSourceCsvImportRequest,
) -> Result<NoteDataSourceCsvImportDto, String> {
    require_uuid(data_source_id, "data_source_id")?;
    if request.csv.len() > MAX_CSV_BYTES {
        return Err("CSV import is limited to 512KB".to_string());
    }
    let dry_run = request.dry_run.unwrap_or(true);
    let parsed = parse_csv(&request.csv)?;
    if !dry_run {
        crate::project_history::ensure_data_source_baseline_for_mutation(pool, data_source_id)
            .await?;
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes CSV import: {e}"))?;
    let (data_source, database) =
        load_active_data_source_and_database_tx(&mut tx, data_source_id).await?;
    let schema_properties = parse_json(&data_source.properties, "data source properties")?;
    let properties = csv_properties(&schema_properties)?;
    let plan = prepare_import_tx(
        &mut tx,
        data_source_id,
        &schema_properties,
        &properties,
        parsed,
        request.has_header.unwrap_or(true),
    )
    .await?;
    let mut imported_page_ids = Vec::new();
    if !dry_run {
        imported_page_ids = write_valid_rows_tx(
            &mut tx,
            data_source_id,
            &database.id,
            &schema_properties,
            &plan.rows,
        )
        .await?;
    }
    tx.commit()
        .await
        .map_err(|e| format!("commit notes CSV import: {e}"))?;
    Ok(plan.into_dto(data_source_id, dry_run, imported_page_ids))
}

async fn prepare_import_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    schema_properties: &Value,
    properties: &[CsvProperty],
    parsed: Vec<Vec<String>>,
    has_header: bool,
) -> Result<CsvImportPlan, String> {
    if parsed.is_empty() {
        return Err("CSV content is empty".to_string());
    }
    let csv_rows = csv_headers_and_rows(parsed, properties, has_header)?;
    if csv_rows.headers.len() > MAX_CSV_COLUMNS {
        return Err("CSV import is limited to 100 columns".to_string());
    }
    if csv_rows.data_rows.len() > MAX_CSV_ROWS {
        return Err("CSV import is limited to 1000 rows".to_string());
    }
    let mappings = map_columns(&csv_rows.headers, properties);
    let mut diagnostics = column_diagnostics(&mappings);
    let mut rows = Vec::new();
    let mut empty_row_count = 0;
    for (row_index, source_row) in csv_rows.data_rows.iter().enumerate() {
        let row_number = (csv_rows.first_data_row_number + row_index) as i64;
        if source_row.iter().all(|cell| cell.trim().is_empty()) {
            empty_row_count += 1;
            diagnostics.push(diagnostic(
                "empty_row",
                "info",
                Some(row_number),
                None,
                None,
                None,
                "Empty CSV row was skipped.",
            ));
            continue;
        }
        let row = prepare_row_tx(
            tx,
            data_source_id,
            schema_properties,
            &mappings,
            source_row,
            row_number,
        )
        .await?;
        diagnostics.extend(row.diagnostics.iter().cloned());
        rows.push(row);
    }
    Ok(CsvImportPlan {
        mappings,
        rows,
        source_row_count: csv_rows.data_rows.len() as i64,
        empty_row_count,
        diagnostics,
    })
}

async fn prepare_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    schema_properties: &Value,
    mappings: &[ColumnMapping],
    source_row: &[String],
    row_number: i64,
) -> Result<PreparedCsvRow, String> {
    let mut properties = Map::new();
    let mut diagnostics = Vec::new();
    let mut title = "Untitled".to_string();
    let mut mapped_cell_count = 0;
    if source_row.len() > mappings.len() {
        diagnostics.push(diagnostic(
            "extra_cells",
            "warning",
            Some(row_number),
            Some((mappings.len() + 1) as i64),
            None,
            None,
            "CSV row has cells beyond the mapped columns. Extra cells were skipped.",
        ));
    }
    for mapping in mappings {
        let Some(property) = mapping.property.as_ref() else {
            continue;
        };
        if mapping.read_only {
            continue;
        }
        let raw = source_row
            .get(mapping.source_index)
            .map(String::as_str)
            .unwrap_or_default();
        if raw.trim().is_empty() && property.property_type != "title" {
            continue;
        }
        match property_value_from_csv(tx, data_source_id, property, raw).await {
            Ok(Some(value)) => {
                if property.property_type == "title" {
                    title = raw.trim().to_string();
                    if title.is_empty() {
                        title = "Untitled".to_string();
                    }
                }
                mapped_cell_count += 1;
                properties.insert(property.key.clone(), value);
            }
            Ok(None) => {}
            Err(message) => diagnostics.push(diagnostic(
                "invalid_cell",
                "error",
                Some(row_number),
                Some((mapping.source_index + 1) as i64),
                Some(mapping.source_name.clone()),
                Some(property.id.clone()),
                &message,
            )),
        }
    }
    if mapped_cell_count == 0 {
        diagnostics.push(diagnostic(
            "empty_mapped_row",
            "error",
            Some(row_number),
            None,
            None,
            None,
            "CSV row does not contain any values for mapped writable properties.",
        ));
    }
    let valid = diagnostics.iter().all(|item| item.severity != "error");
    let normalized = if valid {
        Some(data_sources::rows::row_page_properties(
            schema_properties,
            &title,
            Some(&Value::Object(properties)),
        )?)
    } else {
        None
    };
    let title = normalized
        .as_ref()
        .map(|(title, _)| title.clone())
        .unwrap_or(title);
    let properties = normalized.map(|(_, properties)| properties);
    Ok(PreparedCsvRow {
        row_number,
        title,
        properties,
        mapped_cell_count,
        diagnostics,
    })
}

async fn write_valid_rows_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    database_id: &str,
    schema_properties: &Value,
    rows: &[PreparedCsvRow],
) -> Result<Vec<String>, String> {
    let mut imported_page_ids = Vec::new();
    let mut reserved_ids = HashSet::new();
    for row in rows.iter().filter(|row| row.is_valid()) {
        let Some(properties) = row.properties.as_ref() else {
            continue;
        };
        let page_id = writes::new_note_id(tx, &mut reserved_ids).await?;
        let block_id = writes::new_note_id(tx, &mut reserved_ids).await?;
        insert_row_page_tx(tx, data_source_id, &page_id, row, properties).await?;
        insert_initial_block_tx(tx, &page_id, &block_id, row.row_number).await?;
        data_sources::relations::replace_row_relation_links_tx(
            tx,
            data_source_id,
            &page_id,
            schema_properties,
            properties,
            true,
        )
        .await?;
        imported_page_ids.push(page_id);
    }
    if !imported_page_ids.is_empty() {
        crate::project_history::mark_data_source_dirty_tx(tx, data_source_id, "CSV import", true)
            .await?;
        assets::sync_data_source_property_asset_references_tx(
            tx,
            data_source_id,
            schema_properties,
        )
        .await?;
        data_sources::rollups::invalidate_rollup_cache_for_data_source_tx(tx, data_source_id)
            .await?;
        touch_data_source_and_database_tx(tx, data_source_id, database_id).await?;
    }
    Ok(imported_page_ids)
}

async fn insert_row_page_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    page_id: &str,
    row: &PreparedCsvRow,
    properties: &Value,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO notes_pages (
            id,
            parent_type,
            parent_data_source_id,
            title,
            properties,
            source_provider,
            source_object_id
         )
         VALUES (?, 'data_source_id', ?, ?, ?, 'csv', ?)",
    )
    .bind(page_id)
    .bind(data_source_id)
    .bind(&row.title)
    .bind(properties.to_string())
    .bind(format!("row:{}", row.row_number))
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert CSV data source row page: {e}"))?;
    Ok(())
}

async fn insert_initial_block_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    block_id: &str,
    row_number: i64,
) -> Result<(), String> {
    let payload = writes::default_text_payload("");
    let plain_text = plain_text_from_payload("paragraph", &payload);
    sqlx::query(
        "INSERT INTO notes_blocks (
            id,
            page_id,
            parent_type,
            parent_page_id,
            type,
            payload,
            plain_text,
            sort_order,
            source_provider,
            source_object_id
         )
         VALUES (?, ?, 'page_id', ?, 'paragraph', ?, ?, 1000, 'csv', ?)",
    )
    .bind(block_id)
    .bind(page_id)
    .bind(page_id)
    .bind(payload.to_string())
    .bind(plain_text)
    .bind(format!("row:{}", row_number))
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert CSV data source row block: {e}"))?;
    Ok(())
}

async fn property_value_from_csv(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    property: &CsvProperty,
    raw: &str,
) -> Result<Option<Value>, String> {
    let text = validate_cell_text(raw.trim(), &property.name)?;
    let payload = match property.property_type.as_str() {
        "title" | "rich_text" => {
            if text.is_empty() {
                return Ok(None);
            }
            Value::Array(vec![writes::rich_text(&text)])
        }
        "number" => csv_number_payload(&text)?,
        "checkbox" => Value::Bool(csv_checkbox_payload(&text)?),
        "select" | "status" => csv_single_option_payload(property, &text)?,
        "multi_select" => csv_multi_option_payload(property, &text)?,
        "date" => csv_date_payload(&text)?,
        "url" => csv_url_payload(&text)?,
        "email" => csv_email_payload(&text)?,
        "phone_number" => nullable_text_payload(text),
        "place" => {
            if text.is_empty() {
                Value::Null
            } else {
                json!({ "name": text })
            }
        }
        "relation" => csv_relation_payload(tx, data_source_id, property, &text).await?,
        other => return Err(format!("CSV import does not support {other} properties")),
    };
    if property.property_type == "relation" {
        return Ok(Some(data_sources::relations::relation_property_value(
            &property.id,
            payload,
        )));
    }
    Ok(Some(json!({
        "id": property.id,
        "type": property.property_type,
        property.property_type.clone(): payload
    })))
}

fn csv_headers_and_rows(
    mut rows: Vec<Vec<String>>,
    properties: &[CsvProperty],
    has_header: bool,
) -> Result<CsvRows, String> {
    if has_header {
        let headers = rows.remove(0);
        if headers.iter().all(|header| header.trim().is_empty()) {
            return Err("CSV header row is empty".to_string());
        }
        return Ok(CsvRows {
            headers,
            data_rows: rows,
            first_data_row_number: 2,
        });
    }
    let max_width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let headers = (0..max_width)
        .map(|index| {
            properties
                .get(index)
                .map(|property| property.name.clone())
                .unwrap_or_else(|| format!("Column {}", index + 1))
        })
        .collect();
    Ok(CsvRows {
        headers,
        data_rows: rows,
        first_data_row_number: 1,
    })
}

fn map_columns(headers: &[String], properties: &[CsvProperty]) -> Vec<ColumnMapping> {
    let mut lookup = HashMap::new();
    for property in properties {
        lookup.insert(normalize_key(&property.name), property.clone());
        lookup.insert(normalize_key(&property.id), property.clone());
        lookup.insert(normalize_key(&property.key), property.clone());
    }
    let mut used_property_ids = HashSet::new();
    headers
        .iter()
        .enumerate()
        .map(|(source_index, header)| {
            let source_name = if header.trim().is_empty() {
                format!("Column {}", source_index + 1)
            } else {
                header.trim().to_string()
            };
            let property = lookup.get(&normalize_key(header)).cloned();
            let mut warning = None;
            let mut read_only = false;
            let mut mapped = false;
            let property = property.and_then(|property| {
                if !used_property_ids.insert(property.id.clone()) {
                    warning = Some(
                        "Duplicate CSV column maps to an already mapped property.".to_string(),
                    );
                    return None;
                }
                read_only = READ_ONLY_PROPERTY_TYPES.contains(&property.property_type.as_str());
                mapped = true;
                if read_only {
                    warning = Some(
                        "CSV column maps to a read-only property and will be skipped.".to_string(),
                    );
                }
                Some(property)
            });
            if property.is_none() && warning.is_none() {
                warning = Some(
                    "CSV column does not match a database property and will be skipped."
                        .to_string(),
                );
            }
            ColumnMapping {
                source_index,
                source_name,
                property,
                mapped,
                read_only,
                warning,
            }
        })
        .collect()
}

fn column_diagnostics(mappings: &[ColumnMapping]) -> Vec<NoteDataSourceCsvImportDiagnosticDto> {
    mappings
        .iter()
        .filter_map(|mapping| {
            mapping.warning.as_ref().map(|warning| {
                diagnostic(
                    if mapping.mapped {
                        "read_only_column"
                    } else {
                        "unmapped_column"
                    },
                    "warning",
                    None,
                    Some((mapping.source_index + 1) as i64),
                    Some(mapping.source_name.clone()),
                    mapping
                        .property
                        .as_ref()
                        .map(|property| property.id.clone()),
                    warning,
                )
            })
        })
        .collect()
}

fn csv_properties(schema_properties: &Value) -> Result<Vec<CsvProperty>, String> {
    let object = schema_properties
        .as_object()
        .ok_or_else(|| "data source properties must be an object".to_string())?;
    let mut properties = Vec::with_capacity(object.len());
    for (key, value) in object {
        let property = value
            .as_object()
            .ok_or_else(|| "data source property must be an object".to_string())?;
        properties.push(CsvProperty {
            key: key.clone(),
            id: read_string_field(property, "id", "property.id")?.to_string(),
            name: read_string_field(property, "name", "property.name")?.to_string(),
            property_type: read_string_field(property, "type", "property.type")?.to_string(),
            schema: value.clone(),
        });
    }
    Ok(properties)
}

fn csv_number_payload(text: &str) -> Result<Value, String> {
    if text.is_empty() {
        return Ok(Value::Null);
    }
    let number = text
        .parse::<f64>()
        .map_err(|_| "number value must be a valid number".to_string())?;
    if !number.is_finite() {
        return Err("number value must be finite".to_string());
    }
    Ok(json!(number))
}

fn csv_checkbox_payload(text: &str) -> Result<bool, String> {
    match text.to_ascii_lowercase().as_str() {
        "" | "false" | "0" | "no" | "unchecked" => Ok(false),
        "true" | "1" | "yes" | "checked" => Ok(true),
        _ => Err("checkbox value must be true, false, yes, no, 1, or 0".to_string()),
    }
}

fn csv_single_option_payload(property: &CsvProperty, text: &str) -> Result<Value, String> {
    if text.is_empty() {
        return Ok(Value::Null);
    }
    Ok(Value::Object(option_from_schema(property, text)?))
}

fn csv_multi_option_payload(property: &CsvProperty, text: &str) -> Result<Value, String> {
    if text.is_empty() {
        return Ok(Value::Array(Vec::new()));
    }
    let mut seen = HashSet::new();
    let mut options = Vec::new();
    for item in text
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        let option = option_from_schema(property, item)?;
        let id = option
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if seen.insert(id) {
            options.push(Value::Object(option));
        }
    }
    Ok(Value::Array(options))
}

fn csv_date_payload(text: &str) -> Result<Value, String> {
    if text.is_empty() {
        return Ok(Value::Null);
    }
    if !looks_like_date_or_datetime(text) {
        return Err("date value must be an ISO date or datetime".to_string());
    }
    Ok(json!({ "start": text, "end": null, "time_zone": null }))
}

fn csv_url_payload(text: &str) -> Result<Value, String> {
    if text.is_empty() {
        return Ok(Value::Null);
    }
    let lower = text.to_ascii_lowercase();
    if !(lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("mailto:"))
    {
        return Err("URL value must use HTTP, HTTPS, or mailto".to_string());
    }
    Ok(Value::String(text.to_string()))
}

fn csv_email_payload(text: &str) -> Result<Value, String> {
    if text.is_empty() {
        return Ok(Value::Null);
    }
    if !text.contains('@') || text.chars().any(char::is_whitespace) {
        return Err("email value must look like an email address".to_string());
    }
    Ok(Value::String(text.to_string()))
}

fn nullable_text_payload(text: String) -> Value {
    if !text.is_empty() {
        Value::String(text)
    } else {
        Value::Null
    }
}

async fn csv_relation_payload(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    property: &CsvProperty,
    text: &str,
) -> Result<Value, String> {
    if text.is_empty() {
        return Ok(Value::Array(Vec::new()));
    }
    let target_data_source_id = property
        .schema
        .get("relation")
        .and_then(|value| value.get("data_source_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| "relation property is missing its target data source".to_string())?;
    let mut seen = HashSet::new();
    let mut relation = Vec::new();
    for item in text
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        require_uuid(item, "relation page id")?;
        if !seen.insert(item.to_string()) {
            continue;
        }
        ensure_relation_target_tx(tx, target_data_source_id, item).await?;
        relation.push(json!({ "id": item }));
    }
    if target_data_source_id == data_source_id {
        relation.sort_by(|left, right| {
            left.get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .cmp(right.get("id").and_then(Value::as_str).unwrap_or_default())
        });
    }
    Ok(Value::Array(relation))
}

async fn ensure_relation_target_tx(
    tx: &mut Transaction<'_, Sqlite>,
    target_data_source_id: &str,
    page_id: &str,
) -> Result<(), String> {
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1
         FROM notes_pages AS page
         JOIN notes_data_sources AS data_source ON data_source.id = page.parent_data_source_id
         JOIN notes_databases AS database ON database.id = data_source.database_id
         WHERE page.id = ?
           AND page.parent_type = 'data_source_id'
           AND page.parent_data_source_id = ?
           AND page.in_trash = 0
           AND page.archived = 0
           AND data_source.in_trash = 0
           AND database.in_trash = 0",
    )
    .bind(page_id)
    .bind(target_data_source_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("validate relation target: {e}"))?;
    if exists.is_none() {
        return Err("relation target page must belong to the configured data source".to_string());
    }
    Ok(())
}

fn option_from_schema(
    property: &CsvProperty,
    candidate: &str,
) -> Result<Map<String, Value>, String> {
    let candidate_key = normalize_key(candidate);
    let options = property
        .schema
        .get(&property.property_type)
        .and_then(|config| config.get("options"))
        .and_then(Value::as_array)
        .ok_or_else(|| "option property has no configured options".to_string())?;
    for option in options {
        let Some(object) = option.as_object() else {
            continue;
        };
        let id = object.get("id").and_then(Value::as_str).unwrap_or_default();
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if normalize_key(id) == candidate_key || normalize_key(name) == candidate_key {
            let mut result = Map::new();
            result.insert("id".to_string(), Value::String(id.to_string()));
            result.insert("name".to_string(), Value::String(name.to_string()));
            result.insert(
                "color".to_string(),
                Value::String(
                    object
                        .get("color")
                        .and_then(Value::as_str)
                        .unwrap_or("default")
                        .to_string(),
                ),
            );
            return Ok(result);
        }
    }
    Err("option value must match an existing option".to_string())
}

fn parse_csv(input: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut chars = input.chars().peekable();
    let mut in_quotes = false;
    let mut after_quote = false;
    let mut ended_with_row_break = false;
    while let Some(ch) = chars.next() {
        ended_with_row_break = false;
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    in_quotes = false;
                    after_quote = true;
                }
            } else {
                cell.push(ch);
            }
            continue;
        }
        match ch {
            '"' if cell.is_empty() => in_quotes = true,
            ',' => {
                row.push(std::mem::take(&mut cell));
                after_quote = false;
            }
            '\n' => {
                push_csv_row(&mut rows, &mut row, &mut cell);
                after_quote = false;
                ended_with_row_break = true;
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                push_csv_row(&mut rows, &mut row, &mut cell);
                after_quote = false;
                ended_with_row_break = true;
            }
            ' ' | '\t' if after_quote => {}
            _ if after_quote => return Err("CSV quoted cell has trailing characters".to_string()),
            _ => cell.push(ch),
        }
    }
    if in_quotes {
        return Err("CSV quoted cell is not closed".to_string());
    }
    if !ended_with_row_break || !row.is_empty() || !cell.is_empty() {
        push_csv_row(&mut rows, &mut row, &mut cell);
    }
    Ok(rows)
}

fn push_csv_row(rows: &mut Vec<Vec<String>>, row: &mut Vec<String>, cell: &mut String) {
    row.push(std::mem::take(cell));
    rows.push(std::mem::take(row));
}

fn validate_cell_text(value: &str, label: &str) -> Result<String, String> {
    if value.chars().any(char::is_control) {
        return Err(format!("{label} must not contain control characters"));
    }
    if value.chars().count() > MAX_CELL_CHARS {
        return Err(format!("{label} is too long"));
    }
    Ok(value.to_string())
}

fn looks_like_date_or_datetime(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .take(10)
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

async fn load_active_data_source_and_database_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
) -> Result<(NoteDataSourceRow, NoteDatabaseRow), String> {
    let data_source = sqlx::query_as::<_, NoteDataSourceRow>(
        "SELECT data_source.*
         FROM notes_data_sources AS data_source
         JOIN notes_databases AS database ON database.id = data_source.database_id
         WHERE data_source.id = ?
           AND data_source.in_trash = 0
           AND database.in_trash = 0",
    )
    .bind(data_source_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load CSV import data source: {e}"))?
    .ok_or_else(|| "data source not found".to_string())?;
    let database = sqlx::query_as::<_, NoteDatabaseRow>(
        "SELECT * FROM notes_databases WHERE id = ? AND in_trash = 0",
    )
    .bind(&data_source.database_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("load CSV import database: {e}"))?;
    Ok((data_source, database))
}

async fn touch_data_source_and_database_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    database_id: &str,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE notes_data_sources
         SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(data_source_id.trim())
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("touch CSV import data source: {e}"))?;
    sqlx::query(
        "UPDATE notes_databases
         SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(database_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("touch CSV import database: {e}"))?;
    Ok(())
}

fn diagnostic(
    code: &str,
    severity: &str,
    row_number: Option<i64>,
    column_index: Option<i64>,
    column_name: Option<String>,
    property_id: Option<String>,
    message: &str,
) -> NoteDataSourceCsvImportDiagnosticDto {
    NoteDataSourceCsvImportDiagnosticDto {
        code: code.to_string(),
        severity: severity.to_string(),
        row_number,
        column_index,
        column_name,
        property_id,
        message: message.to_string(),
    }
}

fn normalize_key(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}
fn parse_json(value: &str, label: &str) -> Result<Value, String> {
    serde_json::from_str(value).map_err(|e| format!("parse {label}: {e}"))
}
fn read_string_field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<&'a str, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{label} must be a string"))
}

struct CsvRows {
    headers: Vec<String>,
    data_rows: Vec<Vec<String>>,
    first_data_row_number: usize,
}

#[derive(Clone)]
struct CsvProperty {
    key: String,
    id: String,
    name: String,
    property_type: String,
    schema: Value,
}

struct ColumnMapping {
    source_index: usize,
    source_name: String,
    property: Option<CsvProperty>,
    mapped: bool,
    read_only: bool,
    warning: Option<String>,
}

struct PreparedCsvRow {
    row_number: i64,
    title: String,
    properties: Option<Value>,
    mapped_cell_count: i64,
    diagnostics: Vec<NoteDataSourceCsvImportDiagnosticDto>,
}

impl PreparedCsvRow {
    fn is_valid(&self) -> bool {
        self.properties.is_some() && self.diagnostics.iter().all(|item| item.severity != "error")
    }
}

struct CsvImportPlan {
    mappings: Vec<ColumnMapping>,
    rows: Vec<PreparedCsvRow>,
    source_row_count: i64,
    empty_row_count: i64,
    diagnostics: Vec<NoteDataSourceCsvImportDiagnosticDto>,
}

impl CsvImportPlan {
    fn into_dto(
        self,
        data_source_id: &str,
        dry_run: bool,
        imported_page_ids: Vec<String>,
    ) -> NoteDataSourceCsvImportDto {
        let valid_row_count = self.rows.iter().filter(|row| row.is_valid()).count() as i64;
        let invalid_row_count = self.rows.len() as i64 - valid_row_count;
        NoteDataSourceCsvImportDto {
            object: "notes_data_source_csv_import",
            data_source_id: data_source_id.to_string(),
            dry_run,
            total_row_count: self.source_row_count,
            valid_row_count,
            skipped_row_count: self.empty_row_count + invalid_row_count,
            imported_row_count: imported_page_ids.len() as i64,
            imported_page_ids,
            columns: self
                .mappings
                .into_iter()
                .map(|mapping| NoteDataSourceCsvImportColumnDto {
                    source_index: (mapping.source_index + 1) as i64,
                    source_name: mapping.source_name,
                    property_id: mapping
                        .property
                        .as_ref()
                        .map(|property| property.id.clone()),
                    property_name: mapping
                        .property
                        .as_ref()
                        .map(|property| property.name.clone()),
                    property_type: mapping
                        .property
                        .as_ref()
                        .map(|property| property.property_type.clone()),
                    mapped: mapping.mapped,
                    read_only: mapping.read_only,
                    warning: mapping.warning,
                })
                .collect(),
            rows: self
                .rows
                .into_iter()
                .map(|row| {
                    let valid = row.is_valid();
                    let error_count = row
                        .diagnostics
                        .iter()
                        .filter(|item| item.severity == "error")
                        .count() as i64;
                    NoteDataSourceCsvImportRowDto {
                        row_number: row.row_number,
                        title: row.title,
                        valid,
                        mapped_cell_count: row.mapped_cell_count,
                        error_count,
                    }
                })
                .collect(),
            diagnostics: self.diagnostics,
        }
    }
}
