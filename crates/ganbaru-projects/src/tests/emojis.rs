use super::*;
use crate::emojis::{delete_project_custom_emoji, insert_project_custom_emoji};
use crate::order::OrderPlacement;
use std::fs;

fn emoji_vault(name: &str) -> std::path::PathBuf {
    let vault_root = std::env::temp_dir().join(format!(
        "ganbaru-project-emoji-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&vault_root);
    fs::create_dir_all(vault_root.join("assets/project-icons")).unwrap();
    fs::write(
        vault_root.join("assets/project-icons/abcdef0000000000000000000000000000000000000000000000000000000000.png"),
        [1u8; 42],
    )
    .unwrap();
    vault_root
}

fn emoji(id: &str, asset_path: &str) -> ProjectCustomEmojiCreate {
    ProjectCustomEmojiCreate {
        id: id.to_string(),
        name: " Focus ".to_string(),
        asset_path: asset_path.to_string(),
        placement: OrderPlacement::End,
    }
}

#[test]
fn custom_emoji_create_list_delete_round_trips() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        let vault_root = emoji_vault("round-trip");

        insert_project_custom_emoji(
            &pool,
            &vault_root,
            &emoji(" emoji-a ", " project-icons/abcdef0000000000000000000000000000000000000000000000000000000000.png "),
        )
        .await
        .unwrap();

        let emojis = load_project_custom_emojis(&pool).await.unwrap();
        assert_eq!(emojis.len(), 1);
        assert_eq!(emojis[0].id, "emoji-a");
        assert_eq!(emojis[0].name, "Focus");
        assert_eq!(
            emojis[0].asset_path,
            "project-icons/abcdef0000000000000000000000000000000000000000000000000000000000.png"
        );
        assert_eq!(emojis[0].order_key, "a0");
        let size: Option<i64> = sqlx::query_scalar(
            "SELECT asset_byte_size FROM project_custom_emojis WHERE id = 'emoji-a'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(size, Some(42));

        delete_project_custom_emoji(&pool, " emoji-a ")
            .await
            .unwrap();

        let emojis = load_project_custom_emojis(&pool).await.unwrap();
        assert!(emojis.is_empty());
        assert_eq!(
            delete_project_custom_emoji(&pool, "emoji-a").await,
            Err("project custom emoji not found".to_string())
        );
        let _ = fs::remove_dir_all(&vault_root);
    });
}

#[test]
fn custom_emoji_order_key_places_new_emoji_after_existing_ones_and_refuses_missing_files() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        let vault_root = emoji_vault("order");
        for id in ["emoji-a", "emoji-b"] {
            insert_project_custom_emoji(&pool, &vault_root, &emoji(id, "project-icons/abcdef0000000000000000000000000000000000000000000000000000000000.png"))
                .await
                .unwrap();
        }
        let emojis = load_project_custom_emojis(&pool).await.unwrap();
        assert_eq!(
            emojis
                .iter()
                .map(|emoji| emoji.id.as_str())
                .collect::<Vec<_>>(),
            ["emoji-a", "emoji-b"]
        );
        assert!(emojis[0].order_key < emojis[1].order_key);

        assert!(
            insert_project_custom_emoji(
                &pool,
                &vault_root,
                &emoji("emoji-missing", "project-icons/dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd.png")
            )
            .await
            .is_err()
        );
        let _ = fs::remove_dir_all(&vault_root);
    });
}
