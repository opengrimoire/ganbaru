use super::*;

#[test]
fn project_working_folder_mutations_are_sqlite_backed_and_revision_checked() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let request = CreateProjectWorkingFolderRequest {
            id: ProjectWorkingFolderId::new("workspace-2").unwrap(),
            project_id: "project-chat".to_string(),
            display_name: "Second workspace".to_string(),
        };
        let created = create_workspace(&pool, &request, &UtcTimestamp::new(NOW).unwrap())
            .await
            .unwrap();
        assert_eq!(created.revision, 1);
        let renamed = rename_workspace(
            &pool,
            &request.id,
            "Persistent workspace",
            1,
            &UtcTimestamp::new(NOW).unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(renamed.revision, 2);
        assert_eq!(
            list_workspaces(&pool)
                .await
                .unwrap()
                .iter()
                .filter(|folder| folder.project_id == "project-chat")
                .count(),
            2
        );
        assert!(
            rename_workspace(
                &pool,
                &request.id,
                "Stale rename",
                1,
                &UtcTimestamp::new(NOW).unwrap(),
            )
            .await
            .is_err()
        );
    });
}

#[test]
fn project_archive_preserves_chat_and_project_deletion_removes_linked_threads() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        sqlx::query("UPDATE projects SET status = 'archived' WHERE id = 'project-chat'")
            .execute(&pool)
            .await
            .unwrap();
        let thread_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM chat_threads WHERE project_id = 'project-chat'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(thread_count, 1);
        let folder_archived_at: Option<String> = sqlx::query_scalar(
            "SELECT archived_at FROM project_working_folders WHERE id = 'workspace-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(folder_archived_at, None);
        assert_eq!(
            resolve_project_deletion(
                &pool,
                "project-chat",
                &UtcTimestamp::new(NOW).unwrap(),
                &UtcTimestamp::new(NOW).unwrap(),
            )
            .await
            .unwrap(),
            1
        );
        let remaining_threads: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM chat_threads WHERE project_id = 'project-chat'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining_threads, 0);
        sqlx::query("DELETE FROM projects WHERE id = 'project-chat'")
            .execute(&pool)
            .await
            .unwrap();
    });
}
