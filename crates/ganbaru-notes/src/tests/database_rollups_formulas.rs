use super::helpers::*;

#[test]
fn database_rollups_compute_from_relations_and_invalidate_cache() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_database(
            &pool,
            DATABASE_A,
            DATA_SOURCE_A,
            DATABASE_VIEW_A,
            "Tasks",
            BLOCK_A,
        )
        .await;
        create_database(
            &pool,
            DATABASE_B,
            DATA_SOURCE_B,
            DATABASE_VIEW_B,
            "Projects",
            DATABASE_A,
        )
        .await;
        data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_B,
            None,
            None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": {
                        "id": "title",
                        "name": "Name",
                        "type": "title",
                        "title": {}
                    },
                    "Budget": {
                        "id": "budget",
                        "name": "Budget",
                        "type": "number",
                        "number": {
                            "format": "number"
                        }
                    }
                }),
            },
        )
        .await
        .unwrap();
        data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": {
                        "id": "title",
                        "name": "Name",
                        "type": "title",
                        "title": {}
                    },
                    "Project": {
                        "id": "project_relation",
                        "name": "Project",
                        "type": "relation",
                        "relation": {
                            "data_source_id": DATA_SOURCE_B
                        }
                    },
                    "Project budget": {
                        "id": "project_budget",
                        "name": "Project budget",
                        "type": "rollup",
                        "rollup": {
                            "relation_property_id": "project_relation",
                            "relation_property_name": "Project",
                            "rollup_property_id": "budget",
                            "rollup_property_name": "Budget",
                            "function": "sum"
                        }
                    }
                }),
            },
        )
        .await
        .unwrap();
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_B,
            NoteDataSourceRowPageCreate {
                id: PAGE_C.to_string(),
                title: "Project Alpha".to_string(),
                first_block_id: BLOCK_C.to_string(),
                properties: None,
            },
        )
        .await
        .unwrap();
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_B,
            PAGE_C,
            NoteDataSourceRowPropertyUpdate {
                property_id: "budget".to_string(),
                value: json!(7),
            },
        )
        .await
        .unwrap();
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceRowPageCreate {
                id: PAGE_B.to_string(),
                title: "Write rollup tests".to_string(),
                first_block_id: BLOCK_B.to_string(),
                properties: None,
            },
        )
        .await
        .unwrap();
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceRowPropertyUpdate {
                property_id: "project_relation".to_string(),
                value: json!([PAGE_C]),
            },
        )
        .await
        .unwrap();

        let table =
            data_sources::layouts::table::data_source_table_view(&pool, DATA_SOURCE_A, None, None)
                .await
                .unwrap();
        let table_json = serde_json::to_value(table).unwrap();
        assert_eq!(
            table_json["rows"][0]["properties"]["Project budget"]["rollup"]["number"].as_f64(),
            Some(7.0)
        );
        let stored_properties: String =
            sqlx::query_scalar("SELECT properties FROM notes_pages WHERE id = ?")
                .bind(PAGE_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        let stored_json: serde_json::Value = serde_json::from_str(&stored_properties).unwrap();
        assert!(stored_json.get("Project budget").is_none());
        let cache_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_data_source_rollup_cache")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(cache_count, 1);

        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_B,
            PAGE_C,
            NoteDataSourceRowPropertyUpdate {
                property_id: "budget".to_string(),
                value: json!(11),
            },
        )
        .await
        .unwrap();
        let cache_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_data_source_rollup_cache")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(cache_count, 0);

        let table =
            data_sources::layouts::table::data_source_table_view(&pool, DATA_SOURCE_A, None, None)
                .await
                .unwrap();
        let table_json = serde_json::to_value(table).unwrap();
        assert_eq!(
            table_json["rows"][0]["properties"]["Project budget"]["rollup"]["number"].as_f64(),
            Some(11.0)
        );
    });
}

#[test]
fn database_rollups_reject_incompatible_schema_configuration() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_database(
            &pool,
            DATABASE_A,
            DATA_SOURCE_A,
            DATABASE_VIEW_A,
            "Tasks",
            BLOCK_A,
        )
        .await;
        create_database(
            &pool,
            DATABASE_B,
            DATA_SOURCE_B,
            DATABASE_VIEW_B,
            "Projects",
            DATABASE_A,
        )
        .await;
        let invalid = data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": {
                        "id": "title",
                        "name": "Name",
                        "type": "title",
                        "title": {}
                    },
                    "Project": {
                        "id": "project_relation",
                        "name": "Project",
                        "type": "relation",
                        "relation": {
                            "data_source_id": DATA_SOURCE_B
                        }
                    },
                    "Bad rollup": {
                        "id": "bad_rollup",
                        "name": "Bad rollup",
                        "type": "rollup",
                        "rollup": {
                            "relation_property_id": "project_relation",
                            "relation_property_name": "Project",
                            "rollup_property_id": "title",
                            "rollup_property_name": "Name",
                            "function": "sum"
                        }
                    }
                }),
            },
        )
        .await;
        assert_eq!(
            invalid.err().unwrap(),
            "rollup.function is not compatible with the target property"
        );
    });
}

#[test]
fn database_formulas_compute_without_persisting_stale_values() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_database(
            &pool,
            DATABASE_A,
            DATA_SOURCE_A,
            DATABASE_VIEW_A,
            "Tasks",
            BLOCK_A,
        )
        .await;
        data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": {
                        "id": "title",
                        "name": "Name",
                        "type": "title",
                        "title": {}
                    },
                    "Estimate": {
                        "id": "estimate",
                        "name": "Estimate",
                        "type": "number",
                        "number": {
                            "format": "number"
                        }
                    },
                    "Done": {
                        "id": "done",
                        "name": "Done",
                        "type": "checkbox",
                        "checkbox": {}
                    },
                    "Score": {
                        "id": "score_formula",
                        "name": "Score",
                        "type": "formula",
                        "formula": {
                            "expression": "prop(\"Estimate\") * 2"
                        }
                    },
                    "Score label": {
                        "id": "score_label_formula",
                        "name": "Score label",
                        "type": "formula",
                        "formula": {
                            "expression": "\"Score: \" + prop(\"Score\")"
                        }
                    },
                    "State": {
                        "id": "state_formula",
                        "name": "State",
                        "type": "formula",
                        "formula": {
                            "expression": "if(prop(\"Done\"), \"Complete\", \"Open\")"
                        }
                    },
                    "Broken": {
                        "id": "broken_formula",
                        "name": "Broken",
                        "type": "formula",
                        "formula": {
                            "expression": "prop(\"Estimate\") / 0"
                        }
                    }
                }),
            },
        )
        .await
        .unwrap();
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceRowPageCreate {
                id: PAGE_B.to_string(),
                title: "Write formula tests".to_string(),
                first_block_id: BLOCK_B.to_string(),
                properties: None,
            },
        )
        .await
        .unwrap();
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceRowPropertyUpdate {
                property_id: "estimate".to_string(),
                value: json!(4),
            },
        )
        .await
        .unwrap();
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceRowPropertyUpdate {
                property_id: "done".to_string(),
                value: json!(true),
            },
        )
        .await
        .unwrap();

        let table =
            data_sources::layouts::table::data_source_table_view(&pool, DATA_SOURCE_A, None, None)
                .await
                .unwrap();
        let table_json = serde_json::to_value(table).unwrap();
        let properties = &table_json["rows"][0]["properties"];
        assert_eq!(properties["Score"]["formula"]["type"], "number");
        assert_eq!(properties["Score"]["formula"]["number"].as_f64(), Some(8.0));
        assert_eq!(properties["Score label"]["formula"]["string"], "Score: 8");
        assert_eq!(properties["State"]["formula"]["string"], "Complete");
        assert_eq!(
            properties["Broken"]["formula"]["ganbaru_error"],
            "Broken: division by zero"
        );

        let stored_properties: String =
            sqlx::query_scalar("SELECT properties FROM notes_pages WHERE id = ?")
                .bind(PAGE_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        let stored_json: serde_json::Value = serde_json::from_str(&stored_properties).unwrap();
        assert!(stored_json.get("Score").is_none());
        assert!(stored_json.get("Score label").is_none());
        assert!(stored_json.get("State").is_none());
        assert!(stored_json.get("Broken").is_none());

        let invalid_edit = data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceRowPropertyUpdate {
                property_id: "score_formula".to_string(),
                value: json!(9),
            },
        )
        .await;
        assert_eq!(
            invalid_edit.err().unwrap(),
            "this property is read-only in the table view"
        );
    });
}

#[test]
fn database_formulas_reject_unknown_dependencies_cycles_and_dynamic_prop_calls() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_database(
            &pool,
            DATABASE_A,
            DATA_SOURCE_A,
            DATABASE_VIEW_A,
            "Tasks",
            BLOCK_A,
        )
        .await;

        let unknown_dependency = data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": {
                        "id": "title",
                        "name": "Name",
                        "type": "title",
                        "title": {}
                    },
                    "Bad": {
                        "id": "bad_formula",
                        "name": "Bad",
                        "type": "formula",
                        "formula": {
                            "expression": "prop(\"Missing\")"
                        }
                    }
                }),
            },
        )
        .await;
        assert_eq!(
            unknown_dependency.err().unwrap(),
            "formula references an unknown property"
        );

        let cycle = data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": {
                        "id": "title",
                        "name": "Name",
                        "type": "title",
                        "title": {}
                    },
                    "Alpha": {
                        "id": "alpha_formula",
                        "name": "Alpha",
                        "type": "formula",
                        "formula": {
                            "expression": "prop(\"Beta\") + 1"
                        }
                    },
                    "Beta": {
                        "id": "beta_formula",
                        "name": "Beta",
                        "type": "formula",
                        "formula": {
                            "expression": "prop(\"Alpha\") + 1"
                        }
                    }
                }),
            },
        )
        .await;
        assert_eq!(
            cycle.err().unwrap(),
            "formula dependency cycle is not allowed"
        );

        let dynamic_prop = data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": {
                        "id": "title",
                        "name": "Name",
                        "type": "title",
                        "title": {}
                    },
                    "Bad": {
                        "id": "bad_formula",
                        "name": "Bad",
                        "type": "formula",
                        "formula": {
                            "expression": "prop(format(\"Name\"))"
                        }
                    }
                }),
            },
        )
        .await;
        assert_eq!(
            dynamic_prop.err().unwrap(),
            "prop() requires one literal property name"
        );
    });
}
