use super::helpers::*;

#[test]
fn append_and_update_media_blocks_round_trip() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![
                    block(
                        BLOCK_B,
                        "image",
                        media_payload("https://example.com/image.png", "Cover", None),
                    ),
                    block(
                        BLOCK_C,
                        "video",
                        media_payload("https://www.youtube.com/watch?v=abc123", "", None),
                    ),
                    block(
                        BLOCK_D,
                        "audio",
                        media_payload("https://example.com/song.mp3", "", None),
                    ),
                    block(
                        BLOCK_E,
                        "file",
                        media_payload("https://example.com/doc.txt", "Spec", Some("doc.txt")),
                    ),
                    block(
                        BLOCK_F,
                        "pdf",
                        media_payload("https://example.com/doc.pdf", "", None),
                    ),
                ],
            },
        )
        .await
        .unwrap();

        writes::update_block(
            &pool,
            BLOCK_B,
            block_update(
                "image",
                media_payload("https://example.com/updated.jpg", "Updated cover", None),
            ),
        )
        .await
        .unwrap();

        let stored_image = reads::get_block(&pool, BLOCK_B, false).await.unwrap();
        let stored_image_json = serde_json::to_value(stored_image).unwrap();
        assert_eq!(stored_image_json["type"], "image");
        assert_eq!(
            stored_image_json["image"]["external"]["url"],
            "https://example.com/updated.jpg"
        );
        assert_eq!(
            stored_image_json["image"]["caption"][0]["plain_text"],
            "Updated cover"
        );

        let file_plain_text: String =
            sqlx::query_scalar("SELECT plain_text FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_E)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(file_plain_text, "Spec doc.txt https://example.com/doc.txt");
    });
}

#[test]
fn block_media_asset_references_follow_local_file_payloads() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        let first_asset =
            "notes/files/cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc.png";
        let second_asset =
            "notes/files/dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd.png";

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(
                    BLOCK_B,
                    "image",
                    local_media_payload(
                        first_asset,
                        "image/png",
                        42,
                        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                        "Local cover",
                        Some("cover.png"),
                    ),
                )],
            },
        )
        .await
        .unwrap();

        let first_reference_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM notes_asset_references
             WHERE asset_id = ? AND owner_type = 'block' AND owner_id = ?
                AND page_id = ? AND block_id = ? AND role = 'block_file'",
        )
        .bind(first_asset)
        .bind(BLOCK_B)
        .bind(PAGE_A)
        .bind(BLOCK_B)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(first_reference_count, 1);

        writes::update_block(
            &pool,
            BLOCK_B,
            block_update(
                "image",
                local_media_payload(
                    second_asset,
                    "image/png",
                    84,
                    "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
                    "Replacement",
                    Some("replacement.png"),
                ),
            ),
        )
        .await
        .unwrap();

        let old_reference_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_asset_references WHERE asset_id = ?")
                .bind(first_asset)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(old_reference_count, 0);

        let replacement_reference_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM notes_asset_references
             WHERE asset_id = ? AND owner_type = 'block' AND owner_id = ?
                AND page_id = ? AND block_id = ? AND role = 'block_file'",
        )
        .bind(second_asset)
        .bind(BLOCK_B)
        .bind(PAGE_A)
        .bind(BLOCK_B)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(replacement_reference_count, 1);

        writes::update_block(
            &pool,
            BLOCK_B,
            block_update(
                "image",
                media_payload("https://example.com/replacement.png", "External", None),
            ),
        )
        .await
        .unwrap();

        let remaining_references: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_asset_references WHERE owner_id = ?")
                .bind(BLOCK_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(remaining_references, 0);

        let retained_assets: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_assets")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(retained_assets, 2);
    });
}

#[test]
fn database_file_property_asset_references_follow_local_file_values() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_database(
            &pool,
            DATABASE_A,
            DATA_SOURCE_A,
            DATABASE_VIEW_A,
            "File references",
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
                    "Files": {
                        "id": "files",
                        "name": "Files",
                        "type": "files",
                        "files": {}
                    }
                }),
            },
        )
        .await
        .unwrap();
        let asset_path =
            "notes/files/eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee.pdf";
        let file_value = local_property_file(
            asset_path,
            "application/pdf",
            84,
            "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
            "brief.pdf",
        );

        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceRowPageCreate {
                id: PAGE_B.to_string(),
                title: "With file".to_string(),
                first_block_id: BLOCK_B.to_string(),
                properties: Some(json!({
                    "Files": {
                        "id": "files",
                        "type": "files",
                        "files": [file_value.clone(), file_value]
                    }
                })),
            },
        )
        .await
        .unwrap();

        let reference_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM notes_asset_references
             WHERE asset_id = ?
               AND owner_type = 'data_source_property'
               AND owner_id = ?
               AND data_source_id = ?
               AND property_id = 'files'
               AND role = 'property_file'",
        )
        .bind(asset_path)
        .bind(DATA_SOURCE_A)
        .bind(DATA_SOURCE_A)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(reference_count, 1);

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
                    }
                }),
            },
        )
        .await
        .unwrap();

        let remaining_references: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_asset_references WHERE asset_id = ?")
                .bind(asset_path)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(remaining_references, 0);

        let retained_assets: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_assets WHERE asset_path = ?")
                .bind(asset_path)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(retained_assets, 1);
    });
}
