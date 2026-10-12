use super::helpers::*;

const PROJECT_A: &str = "61616161-6161-4161-8161-616161616161";
const PROJECT_B: &str = "62626262-6262-4262-8262-626262626262";
const FOLDER_A: &str = "71717171-7171-4171-8171-717171717171";
const FOLDER_B: &str = "72727272-7272-4272-8272-727272727272";
const FOLDER_C: &str = "73737373-7373-4373-8373-737373737373";

async fn seed_projects(pool: &SqlitePool) {
    sqlx::query("INSERT INTO project_groups (id, name) VALUES ('notes-folders', 'Notes folders')")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO projects (id, group_id, name)
         VALUES (?, 'notes-folders', 'Project A'), (?, 'notes-folders', 'Project B')",
    )
    .bind(PROJECT_A)
    .bind(PROJECT_B)
    .execute(pool)
    .await
    .unwrap();
}

async fn create_project_page(
    pool: &SqlitePool,
    page_id: &str,
    block_id: &str,
    title: &str,
    folder_id: Option<&str>,
) {
    writes::create_page(
        pool,
        NotePageCreate {
            id: page_id.to_string(),
            title: title.to_string(),
            parent: workspace_parent(),
            folder_id: folder_id.map(str::to_string),
            first_block_id: block_id.to_string(),
            after_block_id: None,
            properties: Some(json!({ "__ganbaru_project_id": PROJECT_A })),
        },
    )
    .await
    .unwrap();
}

async fn create_folder(
    pool: &SqlitePool,
    id: &str,
    project_id: &str,
    parent_folder_id: Option<&str>,
    name: &str,
) {
    folders::create_folder(
        pool,
        NoteFolderCreate {
            id: id.to_string(),
            project_id: project_id.to_string(),
            parent_folder_id: parent_folder_id.map(str::to_string),
            name: name.to_string(),
        },
    )
    .await
    .unwrap();
}

#[test]
fn folders_create_list_update_and_reject_invalid_nesting() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_projects(&pool).await;
        create_folder(&pool, FOLDER_A, PROJECT_A, None, "Research").await;
        create_folder(&pool, FOLDER_B, PROJECT_A, Some(FOLDER_A), "Sources").await;
        create_folder(&pool, FOLDER_C, PROJECT_B, None, "Other project").await;

        let listed = serde_json::to_value(folders::list_folders(&pool).await.unwrap()).unwrap();
        assert_eq!(listed.as_array().unwrap().len(), 3);
        assert_eq!(listed[1]["name"], "Sources");
        assert_eq!(listed[1]["parent_folder_id"], FOLDER_A);

        let updated = folders::update_folder(
            &pool,
            FOLDER_B,
            NoteFolderUpdate {
                parent_folder_id: None,
                name: "References".to_string(),
            },
        )
        .await
        .unwrap();
        let updated = serde_json::to_value(updated).unwrap();
        assert_eq!(updated["name"], "References");
        assert!(updated["parent_folder_id"].is_null());

        let cross_project = folders::update_folder(
            &pool,
            FOLDER_B,
            NoteFolderUpdate {
                parent_folder_id: Some(FOLDER_C.to_string()),
                name: "References".to_string(),
            },
        )
        .await;
        let Err(cross_project) = cross_project else {
            panic!("cross-project folder parent unexpectedly accepted");
        };
        assert_eq!(
            cross_project,
            "parent folder must belong to the same project"
        );

        folders::update_folder(
            &pool,
            FOLDER_B,
            NoteFolderUpdate {
                parent_folder_id: Some(FOLDER_A.to_string()),
                name: "References".to_string(),
            },
        )
        .await
        .unwrap();
        let descendant_move = folders::update_folder(
            &pool,
            FOLDER_A,
            NoteFolderUpdate {
                parent_folder_id: Some(FOLDER_B.to_string()),
                name: "Research".to_string(),
            },
        )
        .await;
        let Err(descendant_move) = descendant_move else {
            panic!("descendant folder parent unexpectedly accepted");
        };
        assert!(descendant_move.contains("cannot be moved under its descendant"));
    });
}

#[test]
fn folder_page_moves_keep_notion_parent_and_child_page_block_consistent() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_projects(&pool).await;
        create_folder(&pool, FOLDER_A, PROJECT_A, None, "Research").await;
        create_project_page(&pool, PAGE_A, BLOCK_A, "Folder page", Some(FOLDER_A)).await;
        create_project_page(&pool, PAGE_B, BLOCK_B, "Parent page", None).await;

        let folder_page = reads::load_page(&pool, PAGE_A).await.unwrap();
        let folder_page = serde_json::to_value(folder_page).unwrap();
        assert_eq!(folder_page["page"]["parent"]["type"], "workspace");
        assert_eq!(folder_page["page"]["folder_id"], FOLDER_A);
        assert!(reads::get_block(&pool, PAGE_A, false).await.is_err());

        let nested = writes::move_page(
            &pool,
            PAGE_A,
            NoteMovePage {
                parent: page_parent(PAGE_B),
                folder_id: None,
            },
        )
        .await
        .unwrap();
        let nested = serde_json::to_value(nested).unwrap();
        assert_eq!(nested["page"]["parent"]["page_id"], PAGE_B);
        assert!(nested["page"]["folder_id"].is_null());
        assert_eq!(
            serde_json::to_value(reads::get_block(&pool, PAGE_A, false).await.unwrap()).unwrap()["type"],
            "child_page"
        );

        let moved_back = writes::move_page(
            &pool,
            PAGE_A,
            NoteMovePage {
                parent: workspace_parent(),
                folder_id: Some(FOLDER_A.to_string()),
            },
        )
        .await
        .unwrap();
        let moved_back = serde_json::to_value(moved_back).unwrap();
        assert_eq!(moved_back["page"]["parent"]["type"], "workspace");
        assert_eq!(moved_back["page"]["folder_id"], FOLDER_A);
        assert!(reads::get_block(&pool, PAGE_A, false).await.is_err());

        let duplicate = writes::duplicate_page(&pool, PAGE_A, NoteDuplicatePage { title: None })
            .await
            .unwrap();
        let duplicate = serde_json::to_value(duplicate).unwrap();
        assert_eq!(duplicate["page"]["parent"]["type"], "workspace");
        assert_eq!(duplicate["page"]["folder_id"], FOLDER_A);
    });
}

#[test]
fn deleting_folder_promotes_children_and_pages_without_deleting_notes() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_projects(&pool).await;
        create_folder(&pool, FOLDER_A, PROJECT_A, None, "Research").await;
        create_folder(&pool, FOLDER_B, PROJECT_A, Some(FOLDER_A), "Sources").await;
        create_project_page(&pool, PAGE_A, BLOCK_A, "Folder page", Some(FOLDER_B)).await;

        assert_eq!(
            folders::delete_folder(&pool, FOLDER_B).await.unwrap(),
            FOLDER_B
        );
        let page_folder: Option<String> =
            sqlx::query_scalar("SELECT folder_id FROM notes_pages WHERE id = ?")
                .bind(PAGE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(page_folder.as_deref(), Some(FOLDER_A));

        folders::delete_folder(&pool, FOLDER_A).await.unwrap();
        let page_folder: Option<String> =
            sqlx::query_scalar("SELECT folder_id FROM notes_pages WHERE id = ?")
                .bind(PAGE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(page_folder, None);
        assert!(reads::load_page(&pool, PAGE_A).await.is_ok());
    });
}
