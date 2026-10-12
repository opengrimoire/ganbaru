use super::helpers::*;

#[test]
fn undo_state_round_trips_and_clears_for_active_page() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        let state_json = r#"{"schema_version":1,"undo":[],"redo":[]}"#;

        undo_state::save_undo_state(&pool, PAGE_A, state_json)
            .await
            .unwrap();
        assert_eq!(
            undo_state::load_undo_state(&pool, PAGE_A).await.unwrap(),
            Some(state_json.to_string())
        );

        undo_state::clear_undo_state(&pool, PAGE_A).await.unwrap();
        assert_eq!(
            undo_state::load_undo_state(&pool, PAGE_A).await.unwrap(),
            None
        );
    });
}

#[test]
fn undo_state_rejects_invalid_or_oversized_json() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        let invalid_error = undo_state::save_undo_state(&pool, PAGE_A, "{bad")
            .await
            .unwrap_err();
        assert!(invalid_error.starts_with("parse notes undo state:"));

        let oversized = format!(r#"{{"payload":"{}"}}"#, "x".repeat(512 * 1024));
        assert_eq!(
            undo_state::save_undo_state(&pool, PAGE_A, &oversized).await,
            Err("notes undo state is too large".to_string())
        );
    });
}

#[test]
fn undo_state_rejects_inactive_pages() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::trash_page(&pool, PAGE_A, true).await.unwrap();

        assert_eq!(
            undo_state::save_undo_state(&pool, PAGE_A, "{}").await,
            Err("notes page not found".to_string())
        );
    });
}
