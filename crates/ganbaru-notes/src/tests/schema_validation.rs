use super::helpers::*;

#[test]
fn schema_rejects_invalid_notes_rows() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        assert!(
            sqlx::query(
                "INSERT INTO notes_blocks (
                id,
                page_id,
                parent_type,
                parent_page_id,
                type,
                payload,
                sort_order
             )
             VALUES (?, ?, 'page_id', ?, 'missing', '{}', 10)",
            )
            .bind(BLOCK_B)
            .bind(PAGE_A)
            .bind(PAGE_A)
            .execute(&pool)
            .await
            .is_err()
        );

        assert!(
            sqlx::query("UPDATE notes_pages SET archived = 2 WHERE id = ?")
                .bind(PAGE_A)
                .execute(&pool)
                .await
                .is_err()
        );

        assert!(
            sqlx::query(
                "INSERT INTO notes_blocks (
                id,
                page_id,
                parent_type,
                parent_page_id,
                type,
                payload,
                sort_order
             )
             VALUES (?, ?, 'page_id', ?, 'paragraph', 'not-json', 10)",
            )
            .bind(PAGE_B)
            .bind(PAGE_A)
            .bind(PAGE_A)
            .execute(&pool)
            .await
            .is_err()
        );
    });
}
