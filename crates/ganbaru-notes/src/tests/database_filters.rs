use super::helpers::*;
use crate::data_sources;
use serde_json::Value;

async fn filter_pool() -> SqlitePool {
    let pool = migrated_memory_pool().await;
    create_page(&pool, PAGE_A, BLOCK_A).await;
    databases::create_database(
        &pool,
        NoteDatabaseCreate {
            id: DATABASE_A.to_string(),
            data_source_id: DATA_SOURCE_A.to_string(),
            view_id: DATABASE_VIEW_A.to_string(),
            title: "Filters".to_string(),
            parent: Some(page_parent(PAGE_A)),
            after_block_id: Some(BLOCK_A.to_string()),
            replace_block_id: None,
            icon: None,
            cover: None,
        },
    )
    .await
    .unwrap();
    data_sources::schema::update_data_source_schema(&pool, DATA_SOURCE_A, None, None, NoteDataSourceSchemaUpdate {
        properties: json!({
            "Name": { "id": "title", "name": "Name", "type": "title", "title": {} },
            "Details": { "id": "details", "name": "Details", "type": "rich_text", "rich_text": {} },
            "Amount's value": { "id": "amount", "name": "Amount's value", "type": "number", "number": { "format": "number" } },
            "Due": { "id": "due", "name": "Due", "type": "date", "date": {} },
            "Done": { "id": "done", "name": "Done", "type": "checkbox", "checkbox": {} },
            "Tags": { "id": "tags", "name": "Tags", "type": "multi_select", "multi_select": { "options": [{ "id": "tag", "name": "Tag", "color": "blue" }] } },
            "Created": { "id": "created", "name": "Created", "type": "created_time", "created_time": {} },
            "Computed": { "id": "computed", "name": "Computed", "type": "formula", "formula": { "expression": "prop(\"Amount's value\") * 2" } }
        }),
    }).await.unwrap();
    for (index, (title, amount, due, checked)) in [
        ("Alpha", json!(2), json!("2026-10-01"), false),
        ("Beta", json!(10), json!("2026-10-02"), true),
        ("Gamma", json!(20), json!("2026-10-03"), false),
        ("Missing", Value::Null, Value::Null, true),
        (
            "Offset",
            json!(10),
            json!("2026-10-02T01:00:00+02:00"),
            false,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let tags = json!([{ "id": "tag", "name": "Tag", "color": "blue" }, { "id": "tag", "name": "Tag", "color": "blue" }]);
        sqlx::query("INSERT INTO notes_pages (id, parent_type, parent_data_source_id, title, properties, created_time) VALUES (?, 'data_source_id', ?, ?, ?, '2026-09-30T12:00:00Z')")
            .bind(format!("60000000-0000-4000-8000-{index:012}"))
            .bind(DATA_SOURCE_A).bind(title).bind(json!({
                "Name": { "id": "title", "type": "title", "title": [{ "type": "text", "plain_text": title, "text": { "content": title, "link": null } }] },
                "Amount's value": { "id": "amount", "type": "number", "number": amount },
                "Due": { "id": "due", "type": "date", "date": if due.is_null() { Value::Null } else { json!({ "start": due, "end": null, "time_zone": null }) } },
                "Done": { "id": "done", "type": "checkbox", "checkbox": checked },
                "Tags": { "id": "tags", "type": "multi_select", "multi_select": tags }
            }).to_string()).execute(&pool).await.unwrap();
    }
    pool
}

fn mixed_filters() -> Value {
    json!([
        { "type": "or", "filters": [
            { "type": "and", "filters": [
                { "property_id": "amount", "condition": "greater_than", "value": 5 },
                { "property_id": "due", "condition": "before", "value": "2026-10-03" }
            ] },
            { "property_id": "title", "condition": "equals", "value": "Alpha" }
        ] },
        { "property_id": "done", "condition": "unchecked", "value": null }
    ])
}

fn table_update(filters: Value) -> NoteDataSourceTableViewUpdate {
    serde_json::from_value(json!({ "filter": filters, "sorts": [], "configuration": {
        "property_order": [], "hidden_property_ids": [], "column_widths": {}, "row_open_mode": "full_page", "group_property_id": "tags"
    } })).unwrap()
}

async fn set_filter_row_title(pool: &SqlitePool, index: usize, title: &str) {
    sqlx::query("UPDATE notes_pages SET title = ?, properties = json_set(properties, '$.Name.title', json(?)) WHERE id = ?")
        .bind(title)
        .bind(json!([{ "type": "text", "plain_text": title, "text": { "content": title, "link": null } }]).to_string())
        .bind(format!("60000000-0000-4000-8000-{index:012}"))
        .execute(pool).await.unwrap();
}

async fn view_csv(pool: &SqlitePool) -> crate::models::NoteDataSourceCsvExportDto {
    data_sources::csv_export::export_csv(
        pool,
        DATA_SOURCE_A,
        NoteDataSourceCsvExportRequest {
            database_id: None,
            view_id: None,
            scope: Some("view".to_string()),
        },
    )
    .await
    .unwrap()
}

#[test]
fn database_filters_match_ascii_case_only_in_sql_and_csv_export() {
    crate::test_block_on(async {
        let pool = filter_pool().await;
        let titles = ["ÁRBOL", "árbol", "alpha", "ALPHA", "Beta"];
        for (index, title) in titles.iter().enumerate() {
            set_filter_row_title(&pool, index, title).await;
        }
        for (condition, value, expected) in [
            ("contains", "Ár", vec!["ÁRBOL"]),
            ("contains", "ár", vec!["árbol"]),
            ("contains", "a", vec!["alpha", "ALPHA", "Beta"]),
            ("equals", "Árbol", vec!["ÁRBOL"]),
            ("equals", "árbol", vec!["árbol"]),
            ("equals", "AlPhA", vec!["alpha", "ALPHA"]),
            (
                "not_equals",
                "Árbol",
                vec!["alpha", "ALPHA", "Beta", "árbol"],
            ),
        ] {
            let table = data_sources::layouts::table::update_data_source_table_view(
                &pool,
                DATA_SOURCE_A,
                None,
                None,
                table_update(
                    json!([{ "property_id": "title", "condition": condition, "value": value }]),
                ),
            )
            .await
            .unwrap();
            let table = serde_json::to_value(table).unwrap();
            let actual = table["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| {
                    row["properties"]["Name"]["title"][0]["plain_text"]
                        .as_str()
                        .unwrap()
                })
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "SQL {condition} {value}");
            assert_eq!(table["total_row_count"], expected.len());
            let export = view_csv(&pool).await;
            let exported = export
                .csv
                .lines()
                .skip(1)
                .map(|line| line.split(',').next().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(exported, expected, "CSV {condition} {value}");
            assert_eq!(export.exported_row_count, expected.len() as i64);
        }
    });
}

#[test]
fn database_filters_use_relation_ids_for_empty_target_titles_in_sql_and_csv_export() {
    crate::test_block_on(async {
        let pool = filter_pool().await;
        let raw_schema: String =
            sqlx::query_scalar("SELECT properties FROM notes_data_sources WHERE id = ?")
                .bind(DATA_SOURCE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        let mut properties: Value = serde_json::from_str(&raw_schema).unwrap();
        properties.as_object_mut().unwrap().insert(
            "Related".to_string(),
            json!({
                "id": "related", "name": "Related", "type": "relation",
                "relation": { "data_source_id": DATA_SOURCE_A }
            }),
        );
        data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceSchemaUpdate { properties },
        )
        .await
        .unwrap();
        set_filter_row_title(&pool, 4, "").await;
        let empty_target_id = "60000000-0000-4000-8000-000000000004";
        for (row_id, target_id) in [
            ("60000000-0000-4000-8000-000000000000", empty_target_id),
            (
                "60000000-0000-4000-8000-000000000001",
                "60000000-0000-4000-8000-000000000002",
            ),
        ] {
            data_sources::layouts::table::update_data_source_row_property(
                &pool,
                DATA_SOURCE_A,
                row_id,
                NoteDataSourceRowPropertyUpdate {
                    property_id: "related".to_string(),
                    value: json!([target_id]),
                },
            )
            .await
            .unwrap();
        }
        for (condition, value, expected) in [
            ("is_empty", Value::Null, vec!["", "Gamma", "Missing"]),
            ("is_not_empty", Value::Null, vec!["Alpha", "Beta"]),
            ("equals", json!(empty_target_id), vec!["Alpha"]),
            ("contains", json!(empty_target_id), vec!["Alpha"]),
            (
                "not_equals",
                json!(empty_target_id),
                vec!["", "Beta", "Gamma", "Missing"],
            ),
            ("equals", json!("Gamma"), vec!["Beta"]),
        ] {
            let table = data_sources::layouts::table::update_data_source_table_view(
                &pool,
                DATA_SOURCE_A,
                None,
                None,
                table_update(
                    json!([{ "property_id": "related", "condition": condition, "value": value }]),
                ),
            )
            .await
            .unwrap();
            let table = serde_json::to_value(table).unwrap();
            let actual = table["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| {
                    row["properties"]["Name"]["title"][0]["plain_text"]
                        .as_str()
                        .unwrap()
                })
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "SQL relation {condition}");
            assert_eq!(table["total_row_count"], expected.len());
            let export = view_csv(&pool).await;
            let exported = export
                .csv
                .lines()
                .skip(1)
                .map(|line| line.split(',').next().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(exported, expected, "CSV relation {condition}");
            assert_eq!(export.exported_row_count, expected.len() as i64);
            assert_eq!(
                export.csv.contains(empty_target_id),
                expected.contains(&"Alpha"),
                "CSV relation uses the ID for the empty target title"
            );
        }
    });
}

#[test]
fn database_filters_use_explicit_unicode_whitespace_for_sql_and_export_emptiness() {
    crate::test_block_on(async {
        let pool = filter_pool().await;
        let whitespace = "\u{0009}\u{000a}\u{000b}\u{000c}\u{000d}\u{0020}\u{0085}\u{00a0}\u{1680}\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}";
        for (index, text) in [whitespace, "\n\t", "", "\u{200b}", "\u{feff}"]
            .into_iter()
            .enumerate()
        {
            let details = json!({ "id": "details", "type": "rich_text", "rich_text":
                if text.is_empty() { json!([]) } else { json!([{ "type": "text", "plain_text": text, "text": { "content": text, "link": null } }]) }
            });
            sqlx::query("UPDATE notes_pages SET properties = json_set(properties, '$.Details', json(?)) WHERE id = ?")
                .bind(details.to_string())
                .bind(format!("60000000-0000-4000-8000-{index:012}"))
                .execute(&pool).await.unwrap();
        }
        for (condition, expected_titles) in [
            ("is_empty", vec!["Alpha", "Beta", "Gamma"]),
            ("is_not_empty", vec!["Missing", "Offset"]),
        ] {
            let table = data_sources::layouts::table::update_data_source_table_view(
                &pool,
                DATA_SOURCE_A,
                None,
                None,
                table_update(
                    json!([{ "property_id": "details", "condition": condition, "value": null }]),
                ),
            )
            .await
            .unwrap();
            let table = serde_json::to_value(table).unwrap();
            let actual = table["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| {
                    row["properties"]["Name"]["title"][0]["plain_text"]
                        .as_str()
                        .unwrap()
                })
                .collect::<Vec<_>>();
            assert_eq!(actual, expected_titles, "SQL {condition}");
            assert_eq!(table["total_row_count"], expected_titles.len());
            let mut update = table_update(
                json!([{ "property_id": "details", "condition": condition, "value": null }]),
            );
            update.configuration.hidden_property_ids = vec!["details".to_string()];
            data_sources::layouts::table::update_data_source_table_view(
                &pool,
                DATA_SOURCE_A,
                None,
                None,
                update,
            )
            .await
            .unwrap();
            let export = view_csv(&pool).await;
            let exported = export
                .csv
                .lines()
                .skip(1)
                .map(|line| line.split(',').next().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(exported, expected_titles, "CSV {condition}");
            assert_eq!(export.exported_row_count, expected_titles.len() as i64);
        }
    });
}

#[test]
fn database_filter_pagination_preserves_accented_and_empty_text_sort_keys() {
    crate::test_block_on(async {
        let pool = filter_pool().await;
        let titles = ["", "Árbol", "École", "árbol", "écume"];
        for (index, title) in titles.iter().enumerate() {
            set_filter_row_title(&pool, index, title).await;
        }
        for direction in ["ascending", "descending"] {
            let mut update = table_update(
                json!([{ "property_id": "title", "condition": "not_equals", "value": "excluded" }]),
            );
            update.sorts = vec![NoteDataSourceTableSort {
                property_id: "title".to_string(),
                direction: direction.to_string(),
            }];
            data_sources::layouts::table::update_data_source_table_view(
                &pool,
                DATA_SOURCE_A,
                None,
                None,
                update,
            )
            .await
            .unwrap();
            let mut expected = titles.to_vec();
            if direction == "descending" {
                expected.reverse();
            }
            let mut actual = Vec::new();
            let mut cursor = None;
            for index in 0..expected.len() {
                let window = data_sources::layouts::table::data_source_table_view_window(
                    &pool,
                    DATA_SOURCE_A,
                    None,
                    None,
                    NoteDataSourceViewWindowRequest {
                        page_size: Some(1),
                        start_cursor: cursor,
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
                let window = serde_json::to_value(window).unwrap();
                assert_eq!(window["total_row_count"], expected.len());
                assert_eq!(
                    window["rows"].as_array().unwrap().len(),
                    1,
                    "page {index} {direction}"
                );
                actual.push(
                    window["rows"][0]["properties"]["Name"]["title"][0]["plain_text"]
                        .as_str()
                        .unwrap()
                        .to_string(),
                );
                assert_eq!(window["has_more"], index + 1 < expected.len());
                cursor = window["next_cursor"].as_str().map(str::to_string);
            }
            assert_eq!(actual, expected, "keyset {direction}");
            let export = view_csv(&pool).await;
            let exported = export
                .csv
                .lines()
                .skip(1)
                .map(|line| line.split(',').next().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(exported, expected, "CSV {direction}");
        }
    });
}

#[test]
fn database_filters_reject_stale_computed_empty_predicates_in_windows_and_view_exports() {
    crate::test_block_on(async {
        let pool = filter_pool().await;
        for condition in ["is_empty", "is_not_empty"] {
            sqlx::query("UPDATE notes_database_views SET filter = ? WHERE id = ?")
                .bind(json!({ "type": "and", "filters": [{ "property_id": "computed", "condition": condition, "value": null }] }).to_string())
                .bind(DATABASE_VIEW_A).execute(&pool).await.unwrap();
            let window_error = data_sources::layouts::table::data_source_table_view_window(
                &pool,
                DATA_SOURCE_A,
                None,
                None,
                NoteDataSourceViewWindowRequest::default(),
            )
            .await
            .err()
            .expect("computed window predicate must be rejected");
            assert!(
                window_error.contains("not supported for formula"),
                "{window_error}"
            );
            let export_error = data_sources::csv_export::export_csv(
                &pool,
                DATA_SOURCE_A,
                NoteDataSourceCsvExportRequest {
                    database_id: None,
                    view_id: None,
                    scope: Some("view".to_string()),
                },
            )
            .await
            .err()
            .expect("computed view export predicate must be rejected");
            assert!(
                export_error.contains("not supported for formula"),
                "{export_error}"
            );
            let all = data_sources::csv_export::export_csv(
                &pool,
                DATA_SOURCE_A,
                NoteDataSourceCsvExportRequest {
                    database_id: None,
                    view_id: None,
                    scope: Some("all".to_string()),
                },
            )
            .await
            .unwrap();
            assert_eq!(
                all.exported_row_count, 5,
                "full export ignores the view query"
            );
        }
    });
}

#[test]
fn database_filters_preserve_boolean_groups_across_all_six_layouts_and_pagination() {
    crate::test_block_on(async {
        let pool = filter_pool().await;
        let filters = mixed_filters();
        let table = data_sources::layouts::table::update_data_source_table_view(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            table_update(filters.clone()),
        )
        .await
        .unwrap();
        let table = serde_json::to_value(table).unwrap();
        assert_eq!(table["total_row_count"], 2);
        assert_eq!(
            table["view"]["filter"],
            json!({ "type": "and", "filters": filters })
        );
        assert_eq!(
            table["group_counts"]["tag"], 2,
            "repeated multivalue membership must count a row once"
        );
        let first = data_sources::layouts::table::data_source_table_view_window(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceViewWindowRequest {
                page_size: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let first = serde_json::to_value(first).unwrap();
        assert_eq!(first["total_row_count"], 2);
        assert_eq!(
            first["rows"][0]["id"],
            "60000000-0000-4000-8000-000000000000"
        );
        assert_eq!(first["has_more"], true);
        let second = data_sources::layouts::table::data_source_table_view_window(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceViewWindowRequest {
                page_size: Some(1),
                start_cursor: Some(first["next_cursor"].as_str().unwrap().to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let second = serde_json::to_value(second).unwrap();
        assert_eq!(
            second["rows"][0]["id"],
            "60000000-0000-4000-8000-000000000004"
        );
        assert_eq!(second["has_more"], false);

        let common = json!({ "group_property_id": "tags", "group_order": [], "hidden_group_ids": [], "visible_property_ids": [], "row_open_mode": "full_page" });
        let board = data_sources::layouts::board::update_data_source_board_view(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            serde_json::from_value(
                json!({ "filter": filters, "sorts": [], "configuration": common }),
            )
            .unwrap(),
        )
        .await
        .unwrap();
        let list = data_sources::layouts::list::update_data_source_list_view(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            serde_json::from_value(
                json!({ "filter": filters, "sorts": [], "configuration": common }),
            )
            .unwrap(),
        )
        .await
        .unwrap();
        let gallery = data_sources::layouts::gallery::update_data_source_gallery_view(&pool, DATA_SOURCE_A, None, None, serde_json::from_value(json!({ "filter": filters, "sorts": [], "configuration": {
            "cover_source": "none", "cover_property_id": null, "visible_property_ids": [], "card_size": "medium", "fit_image": false, "row_open_mode": "full_page"
        } })).unwrap()).await.unwrap();
        let range = json!({ "date_property_id": "due", "group_property_id": "tags", "group_order": [], "hidden_group_ids": [],
            "range_start": "2026-10-01", "range_end": "2026-10-31", "visible_property_ids": [], "row_open_mode": "full_page" });
        let calendar = data_sources::layouts::calendar::update_data_source_calendar_view(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            serde_json::from_value(
                json!({ "filter": filters, "sorts": [], "configuration": range }),
            )
            .unwrap(),
        )
        .await
        .unwrap();
        let timeline = data_sources::layouts::timeline::update_data_source_timeline_view(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            serde_json::from_value(
                json!({ "filter": filters, "sorts": [], "configuration": range }),
            )
            .unwrap(),
        )
        .await
        .unwrap();
        for dto in [
            serde_json::to_value(board).unwrap(),
            serde_json::to_value(list).unwrap(),
            serde_json::to_value(gallery).unwrap(),
            serde_json::to_value(calendar).unwrap(),
            serde_json::to_value(timeline).unwrap(),
        ] {
            assert_eq!(dto["total_row_count"], 2);
            assert_eq!(
                dto["view"]["filter"],
                json!({ "type": "and", "filters": filters })
            );
        }
    });
}

#[test]
fn database_filters_use_numeric_and_date_types_and_reject_invalid_writes_atomically() {
    crate::test_block_on(async {
        let pool = filter_pool().await;
        for (filter, expected) in [
            (
                json!({ "property_id": "amount", "condition": "greater_than", "value": 9 }),
                3,
            ),
            (
                json!({ "property_id": "amount", "condition": "not_equals", "value": 10 }),
                2,
            ),
            (
                json!({ "property_id": "amount", "condition": "is_empty", "value": null }),
                1,
            ),
            (
                json!({ "property_id": "due", "condition": "equals", "value": "2026-10-01" }),
                2,
            ),
            (
                json!({ "property_id": "due", "condition": "on_or_before", "value": "2026-10-02" }),
                3,
            ),
            (
                json!({ "property_id": "due", "condition": "after", "value": "2026-10-01T23:30:00Z" }),
                2,
            ),
            (
                json!({ "property_id": "created", "condition": "equals", "value": "2026-09-30" }),
                5,
            ),
        ] {
            let dto = data_sources::layouts::table::update_data_source_table_view(
                &pool,
                DATA_SOURCE_A,
                None,
                None,
                table_update(json!([filter])),
            )
            .await
            .unwrap();
            assert_eq!(
                serde_json::to_value(dto).unwrap()["total_row_count"],
                expected
            );
        }
        let before: Option<String> =
            sqlx::query_scalar("SELECT filter FROM notes_database_views WHERE id = ?")
                .bind(DATABASE_VIEW_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        for invalid in [
            json!([{ "property_id": "amount", "condition": "greater_than", "value": "10" }]),
            json!([{ "property_id": "amount", "condition": "contains", "value": "10" }]),
            json!([{ "property_id": "due", "condition": "before", "value": "2026-02-30" }]),
            json!([{ "property_id": "done", "condition": "equals", "value": true }]),
            json!([{ "property_id": "computed", "condition": "is_not_empty", "value": null }]),
            json!([{ "type": "or", "filters": [] }]),
            json!([{ "type": "and", "filters": [{ "type": "or", "filters": [{ "type": "and", "filters": [{ "type": "or", "filters": [{ "property_id": "title", "condition": "contains", "value": "a" }] }] }] }] }]),
            json!((0..11).map(|_| json!({ "property_id": "title", "condition": "contains", "value": "a" })).collect::<Vec<_>>()),
            json!((0..9).map(|_| json!({ "type": "and", "filters": [{ "property_id": "title", "condition": "contains", "value": "a" }] })).collect::<Vec<_>>()),
        ] {
            assert!(data_sources::layouts::table::update_data_source_table_view(&pool, DATA_SOURCE_A, None, None, table_update(invalid)).await.is_err());
            let after: Option<String> = sqlx::query_scalar("SELECT filter FROM notes_database_views WHERE id = ?").bind(DATABASE_VIEW_A).fetch_one(&pool).await.unwrap();
            assert_eq!(after, before);
        }
    });
}

#[test]
fn database_filter_reconciliation_and_group_counts_preserve_query_meaning_and_range() {
    crate::test_block_on(async {
        let pool = filter_pool().await;
        let source: String =
            sqlx::query_scalar("SELECT properties FROM notes_data_sources WHERE id = ?")
                .bind(DATA_SOURCE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        let schema =
            data_sources::views::view_schema(&serde_json::from_str(&source).unwrap()).unwrap();
        let window_schema = data_sources::window::table_properties_from_view(&schema);
        let mut tx = pool.begin().await.unwrap();
        let window = data_sources::window::load_row_window_tx(
            &mut tx,
            DATA_SOURCE_A,
            data_sources::window::RowWindowQuery {
                schema: &window_schema,
                filters: &[],
                sorts: &[],
                request: &NoteDataSourceViewWindowRequest {
                    range_start: Some("2026-10-02".to_string()),
                    range_end: Some("2026-10-02T23:59:59Z".to_string()),
                    ..Default::default()
                },
                date_property: window_schema.iter().find(|property| property.id == "due"),
                group_property: window_schema.iter().find(|property| property.id == "tags"),
            },
        )
        .await
        .unwrap();
        assert_eq!(window.total_row_count, 2);
        assert_eq!(window.group_counts.get("tag"), Some(&2));
        tx.commit().await.unwrap();
        let filters: Vec<NoteDataSourceTableFilter> =
            serde_json::from_value(mixed_filters()).unwrap();
        let mut pruned = filters.clone();
        let property_types = [("title", "title"), ("amount", "date"), ("done", "checkbox")]
            .into_iter()
            .collect();
        data_sources::views::reconcile_filters(&mut pruned, &property_types);
        let canonical = data_sources::views::canonical_filter(&pruned, &property_types, "table")
            .unwrap()
            .unwrap();
        assert_eq!(
            canonical,
            json!({ "type": "and", "filters": [
            { "type": "or", "filters": [{ "property_id": "title", "condition": "equals", "value": "Alpha" }] },
            { "property_id": "done", "condition": "unchecked", "value": null }
        ] })
        );
        let raw: crate::models::NotePageRow =
            sqlx::query_as("SELECT * FROM notes_pages WHERE title = 'Alpha'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(data_sources::window::row_matches_filters(
            &raw,
            &window_schema,
            &filters
        ));
    });
}
