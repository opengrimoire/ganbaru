use super::*;
use crate::emojis::{delete_project_custom_emoji, insert_project_custom_emoji};

#[test]
fn custom_emoji_create_list_delete_round_trips() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        let emoji = ProjectCustomEmojiCreate {
            id: " emoji-a ".to_string(),
            name: " Focus ".to_string(),
            asset_path: " project-icons/abcdef.png ".to_string(),
            sort_order: 200,
        };

        insert_project_custom_emoji(&pool, &emoji).await.unwrap();

        let emojis = load_project_custom_emojis(&pool).await.unwrap();
        assert_eq!(emojis.len(), 1);
        assert_eq!(emojis[0].id, "emoji-a");
        assert_eq!(emojis[0].name, "Focus");
        assert_eq!(emojis[0].asset_path, "project-icons/abcdef.png");
        assert_eq!(emojis[0].sort_order, 200);

        delete_project_custom_emoji(&pool, " emoji-a ")
            .await
            .unwrap();

        let emojis = load_project_custom_emojis(&pool).await.unwrap();
        assert!(emojis.is_empty());
        assert_eq!(
            delete_project_custom_emoji(&pool, "emoji-a").await,
            Err("project custom emoji not found".to_string())
        );
    });
}
