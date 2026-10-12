use crate::data_sources::layouts::table::{
    TableProperty, row_property_checked, row_property_number, row_property_plain_text,
};
use crate::data_sources::views::ViewProperty;
use crate::models::{
    NoteDataSourceFilterCondition, NoteDataSourceFilterOperator, NoteDataSourceRowWindow,
    NoteDataSourceTableFilter, NoteDataSourceTableFilterPredicate, NoteDataSourceTableSort,
    NoteDataSourceViewWindowRequest, NotePageRow,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{QueryBuilder, Sqlite, Transaction};
use std::collections::HashMap;

const DEFAULT_WINDOW_SIZE: i64 = 80;
pub(crate) const MAX_WINDOW_SIZE: i64 = 200;
/// Unicode White_Space set shared with the frontend filter evaluator. BOM and
/// zero-width space count as text. SQLite trim needs this explicit set.
const FILTER_WHITESPACE: &str = "\u{0009}\u{000a}\u{000b}\u{000c}\u{000d}\u{0020}\u{0085}\u{00a0}\u{1680}\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}";

#[derive(Clone, Copy)]
pub(crate) struct RowWindowQuery<'a> {
    pub schema: &'a [TableProperty],
    pub filters: &'a [NoteDataSourceTableFilter],
    pub sorts: &'a [NoteDataSourceTableSort],
    pub request: &'a NoteDataSourceViewWindowRequest,
    pub date_property: Option<&'a TableProperty>,
    pub group_property: Option<&'a TableProperty>,
}

pub(crate) fn table_properties_from_view(schema: &[ViewProperty]) -> Vec<TableProperty> {
    schema
        .iter()
        .map(|property| TableProperty {
            key: property.key.clone(),
            id: property.id.clone(),
            name: property.id.clone(),
            property_type: property.property_type.clone(),
            schema: property.schema.clone(),
        })
        .collect()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "value")]
enum CursorValue {
    Null,
    Number(f64),
    Boolean(bool),
    Text(String),
}

#[derive(Deserialize, Serialize)]
struct RowCursor {
    values: Vec<CursorValue>,
    title: String,
    id: String,
}

struct SortExpression<'a> {
    sql: String,
    property: &'a TableProperty,
    descending: bool,
}

pub(crate) async fn load_row_window_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    query_options: RowWindowQuery<'_>,
) -> Result<NoteDataSourceRowWindow, String> {
    let page = load_row_page_tx(tx, data_source_id, query_options).await?;
    let RowWindowQuery {
        schema,
        filters,
        request,
        date_property,
        group_property,
        ..
    } = query_options;
    let mut count_query = QueryBuilder::<Sqlite>::new(
        "SELECT COUNT(*) FROM notes_pages AS page \
         WHERE page.parent_type = 'data_source_id' AND page.parent_data_source_id = ",
    );
    count_query.push_bind(data_source_id);
    count_query.push(" AND page.in_trash = 0 AND page.archived = 0");
    push_filters(&mut count_query, schema, filters);
    push_date_range(&mut count_query, request, date_property);
    let total_row_count: i64 = count_query
        .build_query_scalar()
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| format!("count notes database view rows: {error}"))?;
    let group_counts = load_group_counts_tx(
        tx,
        data_source_id,
        schema,
        filters,
        group_property,
        request,
        date_property,
    )
    .await?;
    Ok(NoteDataSourceRowWindow {
        rows: page.rows,
        total_row_count,
        next_cursor: page.next_cursor,
        has_more: page.has_more,
        group_counts,
    })
}

/// Bounded rows and keyset continuation without complete-source count queries.
pub(crate) struct RowPage {
    pub rows: Vec<NotePageRow>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

/// Read one filtered page for streaming consumers that do not need repeated totals.
pub(crate) async fn load_row_page_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    query_options: RowWindowQuery<'_>,
) -> Result<RowPage, String> {
    let RowWindowQuery {
        schema,
        filters,
        sorts,
        request,
        date_property,
        group_property: _,
    } = query_options;
    let property_types = schema
        .iter()
        .map(|property| (property.id.as_str(), property.property_type.as_str()))
        .collect();
    crate::data_sources::views::canonical_filter(filters, &property_types, "database view")?;
    let page_size = request.page_size.unwrap_or(DEFAULT_WINDOW_SIZE);
    if !(1..=MAX_WINDOW_SIZE).contains(&page_size) {
        return Err(format!(
            "database view page_size must be between 1 and {MAX_WINDOW_SIZE}"
        ));
    }
    let cursor = request
        .start_cursor
        .as_deref()
        .map(|value| {
            serde_json::from_str::<RowCursor>(value)
                .map_err(|e| format!("parse database view cursor: {e}"))
        })
        .transpose()?;
    let sort_expressions = sort_expressions(schema, sorts);
    if let Some(cursor) = &cursor {
        if cursor.values.len() != sort_expressions.len() {
            return Err("database view cursor does not match the active sort".to_string());
        }
    }

    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT page.* FROM notes_pages AS page \
         WHERE page.parent_type = 'data_source_id' AND page.parent_data_source_id = ",
    );
    query.push_bind(data_source_id);
    query.push(" AND page.in_trash = 0 AND page.archived = 0");
    push_filters(&mut query, schema, filters);
    push_date_range(&mut query, request, date_property);
    if let Some(cursor) = &cursor {
        push_cursor_condition(&mut query, &sort_expressions, cursor);
    }
    query.push(" ORDER BY ");
    for (index, sort) in sort_expressions.iter().enumerate() {
        if index > 0 {
            query.push(", ");
        }
        query.push("(").push(&sort.sql).push(") IS NULL ");
        query.push(if sort.descending { "DESC" } else { "ASC" });
        query
            .push(", ")
            .push(&sort.sql)
            .push(if sort.descending { " DESC" } else { " ASC" });
    }
    if !sort_expressions.is_empty() {
        query.push(", ");
    }
    query.push("page.title COLLATE NOCASE ASC, page.id ASC LIMIT ");
    query.push_bind(page_size + 1);
    let built_query = query.build_query_as::<NotePageRow>();
    let query_sql = sqlx::Execute::sql(&built_query).to_string();
    let mut rows = built_query
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| format!("load notes database view row window: {e}; query: {query_sql}"))?;
    let has_more = rows.len() > page_size as usize;
    if has_more {
        rows.truncate(page_size as usize);
    }
    let next_cursor = if has_more {
        rows.last()
            .map(|row| row_cursor(row, &sort_expressions))
            .transpose()?
            .map(|cursor| {
                serde_json::to_string(&cursor)
                    .map_err(|e| format!("serialize database view cursor: {e}"))
            })
            .transpose()?
    } else {
        None
    };
    Ok(RowPage {
        rows,
        next_cursor,
        has_more,
    })
}

async fn load_group_counts_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    schema: &[TableProperty],
    filters: &[NoteDataSourceTableFilter],
    property: Option<&TableProperty>,
    request: &NoteDataSourceViewWindowRequest,
    date_property: Option<&TableProperty>,
) -> Result<HashMap<String, i64>, String> {
    let Some(property) = property else {
        return Ok(HashMap::new());
    };
    let payload = property_value_expression(property);
    let mut query = QueryBuilder::<Sqlite>::new("SELECT ");
    if matches!(
        property.property_type.as_str(),
        "multi_select" | "people" | "relation"
    ) {
        query.push("COALESCE(json_extract(item.value, '$.id'), '__empty__') AS group_id, COUNT(DISTINCT page.id) AS row_count ");
        query
            .push("FROM notes_pages AS page LEFT JOIN json_each(")
            .push(&payload)
            .push(") AS item ");
    } else {
        let group_expression = match property.property_type.as_str() {
            "select" | "status" => {
                format!("COALESCE(json_extract({payload}, '$.id'), '__empty__')")
            }
            "checkbox" => format!("CASE WHEN {payload} = 1 THEN 'true' ELSE 'false' END"),
            "date" => format!("COALESCE(json_extract({payload}, '$.start'), '__empty__')"),
            _ => "'__ungrouped__'".to_string(),
        };
        query
            .push(&group_expression)
            .push(" AS group_id, COUNT(*) AS row_count FROM notes_pages AS page ");
    }
    query.push("WHERE page.parent_type = 'data_source_id' AND page.parent_data_source_id = ");
    query.push_bind(data_source_id);
    query.push(" AND page.in_trash = 0 AND page.archived = 0");
    push_filters(&mut query, schema, filters);
    push_date_range(&mut query, request, date_property);
    query.push(" GROUP BY group_id");
    let rows = query
        .build()
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| format!("group notes database view rows: {e}"))?;
    use sqlx::Row;
    rows.into_iter()
        .map(|row| {
            Ok((
                row.try_get::<String, _>("group_id")?,
                row.try_get::<i64, _>("row_count")?,
            ))
        })
        .collect::<Result<HashMap<_, _>, sqlx::Error>>()
        .map_err(|e| format!("read notes database view group counts: {e}"))
}

fn push_date_range(
    query: &mut QueryBuilder<'_, Sqlite>,
    request: &NoteDataSourceViewWindowRequest,
    date_property: Option<&TableProperty>,
) {
    let (Some(property), Some(range_start), Some(range_end)) = (
        date_property,
        request.range_start.as_deref(),
        request.range_end.as_deref(),
    ) else {
        return;
    };
    let payload = property_value_expression(property);
    query
        .push(" AND json_extract(")
        .push(&payload)
        .push(", '$.start') <= ");
    query.push_bind(range_end.to_string());
    query
        .push(" AND COALESCE(json_extract(")
        .push(&payload)
        .push(", '$.end'), json_extract(");
    query.push(&payload).push(", '$.start')) >= ");
    query.push_bind(range_start.to_string());
}

fn sort_expressions<'a>(
    schema: &'a [TableProperty],
    sorts: &[NoteDataSourceTableSort],
) -> Vec<SortExpression<'a>> {
    sorts
        .iter()
        .filter_map(|sort| {
            let property = schema
                .iter()
                .find(|property| property.id == sort.property_id)?;
            Some(SortExpression {
                sql: property_sort_expression(property),
                property,
                descending: sort.direction == "descending",
            })
        })
        .collect()
}

fn push_filters(
    query: &mut QueryBuilder<'_, Sqlite>,
    schema: &[TableProperty],
    filters: &[NoteDataSourceTableFilter],
) {
    if !filters.is_empty() {
        query.push(" AND ");
        push_filter_nodes(query, schema, filters, NoteDataSourceFilterOperator::And);
    }
}

fn push_filter_nodes(
    query: &mut QueryBuilder<'_, Sqlite>,
    schema: &[TableProperty],
    filters: &[NoteDataSourceTableFilter],
    operator: NoteDataSourceFilterOperator,
) {
    query.push("(");
    for (index, filter) in filters.iter().enumerate() {
        if index > 0 {
            query.push(if operator == NoteDataSourceFilterOperator::And {
                " AND "
            } else {
                " OR "
            });
        }
        match filter {
            NoteDataSourceTableFilter::Group(group) => {
                push_filter_nodes(query, schema, &group.filters, group.operator)
            }
            NoteDataSourceTableFilter::Predicate(predicate) => {
                let Some(property) = schema
                    .iter()
                    .find(|property| property.id == predicate.property_id)
                else {
                    query.push("0");
                    continue;
                };
                push_filter_predicate(query, property, predicate);
            }
        }
    }
    query.push(")");
}

fn comparison_operator(condition: NoteDataSourceFilterCondition) -> &'static str {
    use NoteDataSourceFilterCondition::*;
    match condition {
        NotEquals => " <> ",
        GreaterThan | After => " > ",
        GreaterThanOrEqual | OnOrAfter => " >= ",
        LessThan | Before => " < ",
        LessThanOrEqual | OnOrBefore => " <= ",
        _ => " = ",
    }
}

fn push_filter_predicate(
    query: &mut QueryBuilder<'_, Sqlite>,
    property: &TableProperty,
    predicate: &NoteDataSourceTableFilterPredicate,
) {
    use NoteDataSourceFilterCondition::*;
    let expression = property_text_expression(property);
    match predicate.condition {
        Contains => {
            query
                .push("instr(lower(")
                .push(&expression)
                .push("), lower(");
            query
                .push_bind(
                    predicate
                        .value
                        .as_ref()
                        .map(scalar_filter_text)
                        .unwrap_or_default(),
                )
                .push(")) > 0");
        }
        IsEmpty | IsNotEmpty => {
            query
                .push("trim(")
                .push(&expression)
                .push(", ")
                .push_bind(FILTER_WHITESPACE)
                .push(")");
            query.push(if predicate.condition == IsEmpty {
                " = ''"
            } else {
                " <> ''"
            });
        }
        Checked | Unchecked => {
            query
                .push(property_value_expression(property))
                .push(" = ")
                .push_bind(predicate.condition == Checked);
        }
        condition if property.property_type == "number" => {
            let payload = property_value_expression(property);
            query
                .push("(CASE WHEN json_type(page.properties, '")
                .push(sql_string(&json_path(
                    &property.key,
                    &property.property_type,
                )))
                .push("') IN ('integer', 'real') THEN CAST(")
                .push(payload)
                .push(" AS REAL) END)")
                .push(comparison_operator(condition))
                .push_bind(predicate.value.as_ref().and_then(Value::as_f64));
        }
        condition
            if matches!(
                property.property_type.as_str(),
                "date" | "created_time" | "last_edited_time"
            ) =>
        {
            let value = predicate
                .value
                .as_ref()
                .and_then(Value::as_str)
                .unwrap_or_default();
            let day_only = value.len() == 10;
            query
                .push(if day_only {
                    "julianday(date("
                } else {
                    "julianday("
                })
                .push(&expression)
                .push(if day_only { "))" } else { ")" })
                .push(comparison_operator(condition))
                .push("julianday(")
                .push_bind(value.to_string())
                .push(")");
        }
        condition => {
            query
                .push("lower(")
                .push(&expression)
                .push(")")
                .push(comparison_operator(condition))
                .push("lower(");
            query
                .push_bind(
                    predicate
                        .value
                        .as_ref()
                        .map(scalar_filter_text)
                        .unwrap_or_default(),
                )
                .push(")");
        }
    }
}

/// Evaluate export predicates with the same Boolean and scalar semantics as row-window SQL.
pub(crate) fn row_matches_filters(
    row: &NotePageRow,
    schema: &[TableProperty],
    filters: &[NoteDataSourceTableFilter],
) -> bool {
    filters
        .iter()
        .all(|filter| row_matches_filter(row, schema, filter))
}

fn row_matches_filter(
    row: &NotePageRow,
    schema: &[TableProperty],
    filter: &NoteDataSourceTableFilter,
) -> bool {
    use NoteDataSourceFilterCondition::*;
    let predicate = match filter {
        NoteDataSourceTableFilter::Group(group) => {
            return match group.operator {
                NoteDataSourceFilterOperator::And => group
                    .filters
                    .iter()
                    .all(|node| row_matches_filter(row, schema, node)),
                NoteDataSourceFilterOperator::Or => group
                    .filters
                    .iter()
                    .any(|node| row_matches_filter(row, schema, node)),
            };
        }
        NoteDataSourceTableFilter::Predicate(predicate) => predicate,
    };
    let Some(property) = schema
        .iter()
        .find(|property| property.id == predicate.property_id)
    else {
        return false;
    };
    if !crate::data_sources::views::filter_condition_supported(
        &property.property_type,
        predicate.condition,
    ) {
        return false;
    }
    let text = row_property_plain_text(row, property);
    let value = predicate
        .value
        .as_ref()
        .map(scalar_filter_text)
        .unwrap_or_default();
    match predicate.condition {
        Contains => text
            .to_ascii_lowercase()
            .contains(&value.to_ascii_lowercase()),
        IsEmpty => text
            .chars()
            .all(|character| FILTER_WHITESPACE.contains(character)),
        IsNotEmpty => text
            .chars()
            .any(|character| !FILTER_WHITESPACE.contains(character)),
        Checked => row_property_checked(row, property) == Some(true),
        Unchecked => row_property_checked(row, property) == Some(false),
        condition => {
            let ordering = if property.property_type == "number" {
                row_property_number(row, property)
                    .zip(predicate.value.as_ref().and_then(Value::as_f64))
                    .and_then(|(left, right)| left.partial_cmp(&right))
            } else if matches!(
                property.property_type.as_str(),
                "date" | "created_time" | "last_edited_time"
            ) {
                filter_date_number(&text, value.len() == 10)
                    .zip(filter_date_number(&value, value.len() == 10))
                    .map(|(left, right)| left.cmp(&right))
            } else {
                Some(text.to_ascii_lowercase().cmp(&value.to_ascii_lowercase()))
            };
            ordering.is_some_and(|ordering| match condition {
                Equals => ordering.is_eq(),
                NotEquals => !ordering.is_eq(),
                GreaterThan | After => ordering.is_gt(),
                GreaterThanOrEqual | OnOrAfter => !ordering.is_lt(),
                LessThan | Before => ordering.is_lt(),
                LessThanOrEqual | OnOrBefore => !ordering.is_gt(),
                _ => false,
            })
        }
    }
}

fn filter_date_number(value: &str, day_only: bool) -> Option<i64> {
    let timestamp = if value.len() == 10 {
        chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .ok()?
            .and_hms_opt(0, 0, 0)?
            .and_utc()
            .timestamp_millis()
    } else if let Ok(date) = chrono::DateTime::parse_from_rfc3339(value) {
        date.timestamp_millis()
    } else {
        chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
            .ok()?
            .and_utc()
            .timestamp_millis()
    };
    Some(if day_only {
        timestamp.div_euclid(86_400_000)
    } else {
        timestamp
    })
}

fn push_cursor_condition(
    query: &mut QueryBuilder<'_, Sqlite>,
    sorts: &[SortExpression<'_>],
    cursor: &RowCursor,
) {
    query.push(" AND (");
    let mut prior_equalities: Vec<(String, CursorValue)> = Vec::new();
    for (sort, value) in sorts.iter().zip(&cursor.values) {
        push_cursor_branch(query, &prior_equalities, &sort.sql, sort.descending, value);
        query.push(" OR ");
        prior_equalities.push((sort.sql.clone(), value.clone()));
    }
    push_prior_equalities(query, &prior_equalities);
    if !prior_equalities.is_empty() {
        query.push(" AND ");
    }
    query
        .push("(page.title COLLATE NOCASE > ")
        .push_bind(cursor.title.clone())
        .push(" OR (page.title COLLATE NOCASE = ");
    query.push_bind(cursor.title.clone());
    query
        .push(" AND page.id > ")
        .push_bind(cursor.id.clone())
        .push("))");
    query.push(")");
}

fn push_cursor_branch(
    query: &mut QueryBuilder<'_, Sqlite>,
    prior: &[(String, CursorValue)],
    expression: &str,
    descending: bool,
    value: &CursorValue,
) {
    query.push("(");
    push_prior_equalities(query, prior);
    if !prior.is_empty() {
        query.push(" AND ");
    }
    let cursor_is_null = matches!(value, CursorValue::Null);
    query.push("(").push(expression).push(") IS NULL ");
    query
        .push(if descending { "< " } else { "> " })
        .push_bind(cursor_is_null);
    query
        .push(" OR ((")
        .push(expression)
        .push(") IS NULL = ")
        .push_bind(cursor_is_null);
    query
        .push(" AND ")
        .push(expression)
        .push(if descending { " < " } else { " > " });
    push_cursor_value(query, value);
    query.push(")");
    query.push(")");
}

fn push_prior_equalities(query: &mut QueryBuilder<'_, Sqlite>, prior: &[(String, CursorValue)]) {
    for (index, (expression, value)) in prior.iter().enumerate() {
        if index > 0 {
            query.push(" AND ");
        }
        if matches!(value, CursorValue::Null) {
            query.push("(").push(expression).push(") IS NULL");
        } else {
            query.push("(").push(expression).push(") = ");
            push_cursor_value(query, value);
        }
    }
}

fn push_cursor_value(query: &mut QueryBuilder<'_, Sqlite>, value: &CursorValue) {
    match value {
        CursorValue::Null => {
            query.push("NULL");
        }
        CursorValue::Number(value) => {
            query.push_bind(*value);
        }
        CursorValue::Boolean(value) => {
            query.push_bind(*value);
        }
        CursorValue::Text(value) => {
            query.push_bind(value.clone());
        }
    }
}

fn row_cursor(row: &NotePageRow, sorts: &[SortExpression<'_>]) -> Result<RowCursor, String> {
    Ok(RowCursor {
        values: sorts
            .iter()
            .map(|sort| cursor_value(row, sort.property))
            .collect(),
        title: row.title.clone(),
        id: row.id.clone(),
    })
}

fn cursor_value(row: &NotePageRow, property: &TableProperty) -> CursorValue {
    match property.property_type.as_str() {
        "number" | "rollup" | "formula" => row_property_number(row, property)
            .map(CursorValue::Number)
            .unwrap_or(CursorValue::Null),
        "checkbox" => row_property_checked(row, property)
            .map(CursorValue::Boolean)
            .unwrap_or(CursorValue::Null),
        _ => CursorValue::Text(row_property_plain_text(row, property).to_ascii_lowercase()),
    }
}

fn property_sort_expression(property: &TableProperty) -> String {
    match property.property_type.as_str() {
        "number" => format!("CAST({} AS REAL)", property_value_expression(property)),
        "checkbox" => format!("CAST({} AS INTEGER)", property_value_expression(property)),
        _ => format!("lower({})", property_text_expression(property)),
    }
}

fn property_text_expression(property: &TableProperty) -> String {
    let payload = property_value_expression(property);
    match property.property_type.as_str() {
        "title" => "COALESCE(page.title, '')".to_string(),
        "rich_text" => format!(
            "COALESCE((SELECT group_concat(COALESCE(json_extract(item.value, '$.plain_text'), json_extract(item.value, '$.text.content'), ''), '') FROM json_each({payload}) AS item), '')"
        ),
        "number" | "checkbox" | "url" | "email" | "phone_number" => {
            format!("COALESCE(CAST({payload} AS TEXT), '')")
        }
        "select" | "status" | "place" => format!("COALESCE(json_extract({payload}, '$.name'), '')"),
        "multi_select" => format!(
            "COALESCE((SELECT group_concat(COALESCE(json_extract(item.value, '$.name'), ''), ', ') FROM json_each({payload}) AS item), '')"
        ),
        "people" | "files" => format!(
            "COALESCE((SELECT group_concat(COALESCE(NULLIF(json_extract(item.value, '$.name'), ''), json_extract(item.value, '$.id'), ''), ', ') FROM json_each({payload}) AS item), '')"
        ),
        "relation" => format!(
            "COALESCE((SELECT group_concat(COALESCE(NULLIF(target.title, ''), target.id), ', ') \
              FROM notes_data_source_relation_links AS relation \
              JOIN notes_pages AS target ON target.id = relation.target_page_id \
              WHERE relation.source_page_id = page.id \
                AND relation.source_property_id = '{}'), '')",
            sql_string(&property.id),
        ),
        "rollup" => format!(
            "COALESCE((SELECT CAST(cache.value AS TEXT) \
              FROM notes_data_source_rollup_cache AS cache \
              WHERE cache.source_page_id = page.id \
                AND cache.source_property_id = '{}'), '')",
            sql_string(&property.id),
        ),
        "date" => format!("COALESCE(json_extract({payload}, '$.start'), '')"),
        "created_time" => "page.created_time".to_string(),
        "last_edited_time" => "page.last_edited_time".to_string(),
        "created_by" | "last_edited_by" => format!(
            "COALESCE(NULLIF(json_extract({payload}, '$.name'), ''), json_extract({payload}, '$.id'), '')"
        ),
        "unique_id" => format!(
            "COALESCE(json_extract({payload}, '$.prefix'), '') || COALESCE(CAST(json_extract({payload}, '$.number') AS TEXT), '')"
        ),
        _ => format!("COALESCE(CAST({payload} AS TEXT), '')"),
    }
}

fn sql_string(value: &str) -> String {
    value.replace('\'', "''")
}

fn property_value_expression(property: &TableProperty) -> String {
    format!(
        "json_extract(page.properties, '{}')",
        sql_string(&json_path(&property.key, &property.property_type)),
    )
}

fn json_path(key: &str, property_type: &str) -> String {
    let escaped_key = key.replace('\\', "\\\\").replace('"', "\\\"");
    let escaped_type = property_type.replace('\\', "\\\\").replace('"', "\\\"");
    format!("$.\"{escaped_key}\".\"{escaped_type}\"")
}

fn scalar_filter_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        _ => String::new(),
    }
}
