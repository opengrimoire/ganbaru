use super::*;
use crate::preferences::upsert_view_preference;
use crate::views::{create_view, delete_view, rename_view, reorder_views};
use crate::workspace::load_optional_data;

async fn view_order(pool: &SqlitePool, project_id: &str) -> Vec<(String, String, Option<String>)> {
    sqlx::query_as(
        "SELECT id, view_type, name FROM project_views WHERE project_id = ? ORDER BY order_key, id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
    .unwrap()
}

fn view_create(id: &str, view_type: &str, name: Option<&str>) -> ProjectViewCreate {
    ProjectViewCreate {
        id: id.to_string(),
        project_id: "project-a".to_string(),
        view_type: view_type.to_string(),
        name: name.map(str::to_string),
    }
}

#[test]
fn views_append_repeat_task_types_and_keep_one_notes_and_chat_view() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;

        create_view(&pool, view_create("view-list", "list", None))
            .await
            .unwrap();
        let created = create_view(
            &pool,
            view_create("view-week", "list", Some("  This   week ")),
        )
        .await
        .unwrap();
        assert_eq!(created.views[0].name.as_deref(), Some("This week"));
        create_view(&pool, view_create("view-notes", "notes", None))
            .await
            .unwrap();

        assert_eq!(
            create_view(&pool, view_create("view-notes-2", "notes", None))
                .await
                .err()
                .as_deref(),
            Some("project already has a notes view"),
        );
        assert!(
            create_view(&pool, view_create("view-web", "web", None))
                .await
                .is_err()
        );
        assert_eq!(
            view_order(&pool, "project-a").await,
            [
                ("view-list".to_string(), "list".to_string(), None),
                (
                    "view-week".to_string(),
                    "list".to_string(),
                    Some("This week".to_string())
                ),
                ("view-notes".to_string(), "notes".to_string(), None),
            ],
        );
    });
}

#[test]
fn renaming_a_view_to_a_blank_name_restores_its_type_label() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        create_view(&pool, view_create("view-list", "list", None))
            .await
            .unwrap();

        let renamed = rename_view(
            &pool,
            ProjectViewRename {
                id: "view-list".to_string(),
                name: Some("Backlog".to_string()),
            },
        )
        .await
        .unwrap();
        assert_eq!(renamed.views[0].name.as_deref(), Some("Backlog"));

        let reset = rename_view(
            &pool,
            ProjectViewRename {
                id: "view-list".to_string(),
                name: Some("   ".to_string()),
            },
        )
        .await
        .unwrap();
        assert_eq!(reset.views[0].name, None);
        assert!(
            rename_view(
                &pool,
                ProjectViewRename {
                    id: "missing".to_string(),
                    name: None
                }
            )
            .await
            .is_err()
        );
    });
}

#[test]
fn reordering_views_requires_the_complete_order() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        for (id, view_type) in [
            ("view-a", "dashboard"),
            ("view-b", "list"),
            ("view-c", "chat"),
        ] {
            create_view(&pool, view_create(id, view_type, None))
                .await
                .unwrap();
        }
        let reorder = |ids: &[&str]| ProjectViewReorder {
            project_id: "project-a".to_string(),
            view_ids: ids.iter().map(|id| id.to_string()).collect(),
        };

        for incomplete in [
            &["view-c", "view-a"][..],
            &["view-c", "view-a", "view-a"],
            &["view-c", "view-a", "other"],
        ] {
            assert!(reorder_views(&pool, reorder(incomplete)).await.is_err());
        }
        let mutation = reorder_views(&pool, reorder(&["view-c", "view-a", "view-b"]))
            .await
            .unwrap();
        let ids: Vec<&str> = mutation.views.iter().map(|view| view.id.as_str()).collect();
        assert_eq!(ids, ["view-c", "view-a", "view-b"]);
        let orders: Vec<&str> = mutation
            .views
            .iter()
            .map(|view| view.order_key.as_str())
            .collect();
        assert!(
            orders.windows(2).all(|pair| pair[0] < pair[1]),
            "{orders:?}"
        );
    });
}

#[test]
fn view_preferences_belong_to_one_view_and_leave_with_it() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        create_view(&pool, view_create("view-a", "list", None))
            .await
            .unwrap();
        create_view(&pool, view_create("view-b", "list", None))
            .await
            .unwrap();
        for (view_id, value) in [("view-a", "[\"due\"]"), ("view-b", "[\"priority\"]")] {
            upsert_view_preference(
                &pool,
                ProjectViewPreferenceUpsert {
                    view_id: view_id.to_string(),
                    preference_key: "list-visible-columns".to_string(),
                    preference_value: value.to_string(),
                },
            )
            .await
            .unwrap();
        }
        assert!(
            upsert_view_preference(
                &pool,
                ProjectViewPreferenceUpsert {
                    view_id: "missing".to_string(),
                    preference_key: "list-visible-columns".to_string(),
                    preference_value: "[]".to_string(),
                },
            )
            .await
            .is_err()
        );

        let loaded = load_optional_data(
            &pool,
            Some("project-a"),
            ProjectOptionalDataKind::ViewPreferences,
        )
        .await
        .unwrap();
        let rows: Vec<(&str, &str, &str)> = loaded
            .view_preferences
            .iter()
            .map(|row| {
                (
                    row.project_id.as_str(),
                    row.view_id.as_str(),
                    row.preference_value.as_str(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("project-a", "view-a", "[\"due\"]"),
                ("project-a", "view-b", "[\"priority\"]")
            ],
        );

        let removed = delete_view(&pool, "view-a").await.unwrap();
        assert!(matches!(
            removed.removals.as_slice(),
            [ProjectMutationRemoval::View { id }] if id == "view-a"
        ));
        let remaining: Vec<String> =
            sqlx::query_scalar("SELECT view_id FROM project_view_preferences ORDER BY view_id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, ["view-b"]);
        assert!(delete_view(&pool, "view-a").await.is_err());
    });
}
