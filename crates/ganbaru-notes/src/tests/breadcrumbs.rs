use super::helpers::*;

#[test]
fn page_open_returns_visible_chrome_and_top_level_blocks_only() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_A),
                after: None,
                children: vec![block(BLOCK_B, "paragraph", paragraph_payload("Nested"))],
            },
        )
        .await
        .unwrap();

        let opened = serde_json::to_value(reads::open_page(&pool, PAGE_A).await.unwrap()).unwrap();

        assert_eq!(opened["page"]["id"], PAGE_A);
        assert_eq!(opened["breadcrumb"][0]["id"], PAGE_A);
        assert_eq!(opened["blocks"]["results"].as_array().unwrap().len(), 1);
        assert_eq!(opened["blocks"]["results"][0]["id"], BLOCK_A);
        assert_eq!(opened["outlines"][0]["id"], BLOCK_A);
        assert_eq!(opened["outlines"][0]["retained_height"], 36);
        assert!(
            opened["blocks"]["results"]
                .as_array()
                .unwrap()
                .iter()
                .all(|block| block["id"] != BLOCK_B)
        );
    });
}

#[test]
fn block_frontier_batches_children_for_multiple_parents() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "toggle", paragraph_payload("Second"))],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_A),
                after: None,
                children: vec![block(
                    BLOCK_C,
                    "paragraph",
                    paragraph_payload("First child"),
                )],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(
                    BLOCK_D,
                    "paragraph",
                    paragraph_payload("Second child"),
                )],
            },
        )
        .await
        .unwrap();

        let frontier = reads::block_frontier(&pool, &[BLOCK_A.to_string(), BLOCK_B.to_string()])
            .await
            .unwrap();
        let json = serde_json::to_value(frontier).unwrap();
        let ids = json["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|block| block["id"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![BLOCK_C, BLOCK_D]);

        let outlines = reads::block_outline_frontier(
            &pool,
            PAGE_A,
            &[BLOCK_A.to_string(), BLOCK_B.to_string()],
        )
        .await
        .unwrap();
        let outline_json = serde_json::to_value(outlines).unwrap();
        assert_eq!(outline_json[0]["id"], BLOCK_C);
        assert!(outline_json[0].get("paragraph").is_none());

        let hydrated = reads::hydrate_blocks(
            &pool,
            NoteBlockHydrationRequest {
                page_id: PAGE_A.to_string(),
                block_ids: vec![BLOCK_D.to_string()],
            },
        )
        .await
        .unwrap();
        let hydrated_json = serde_json::to_value(hydrated).unwrap();
        assert_eq!(hydrated_json.as_array().unwrap().len(), 1);
        assert_eq!(hydrated_json[0]["id"], BLOCK_D);
        assert_eq!(
            hydrated_json[0]["paragraph"]["rich_text"][0]["plain_text"],
            "Second child"
        );
    });
}

#[test]
fn block_frontier_reads_accept_the_parent_cap_and_refuse_one_more() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_A),
                after: None,
                children: vec![block(BLOCK_B, "paragraph", paragraph_payload("Child"))],
            },
        )
        .await
        .unwrap();
        let mut parent_ids = (1..reads::MAX_BLOCK_FRONTIER_PARENT_IDS)
            .map(|index| format!("00000000-0000-4000-8000-{index:012x}"))
            .collect::<Vec<_>>();
        parent_ids.push(BLOCK_A.to_string());

        let frontier =
            serde_json::to_value(reads::block_frontier(&pool, &parent_ids).await.unwrap()).unwrap();
        assert_eq!(frontier["blocks"][0]["id"], BLOCK_B);
        let outlines = reads::block_outline_frontier(&pool, PAGE_A, &parent_ids)
            .await
            .unwrap();
        assert_eq!(serde_json::to_value(outlines).unwrap()[0]["id"], BLOCK_B);

        parent_ids.push(BLOCK_C.to_string());
        let error = reads::block_frontier(&pool, &parent_ids)
            .await
            .err()
            .unwrap();
        assert!(error.contains("limited to"), "{error}");
        let error = reads::block_outline_frontier(&pool, PAGE_A, &parent_ids)
            .await
            .unwrap_err();
        assert!(error.contains("limited to"), "{error}");
    });
}

#[test]
fn page_breadcrumb_resolves_unloaded_ancestor_rows() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        writes::create_page(
            &pool,
            NotePageCreate {
                id: PAGE_A.to_string(),
                title: "Root".to_string(),
                parent: workspace_parent(),
                folder_id: None,
                project_id: None,
                first_block_id: BLOCK_A.to_string(),
                after_block_id: None,
                properties: None,
            },
        )
        .await
        .unwrap();
        writes::create_page(
            &pool,
            NotePageCreate {
                id: PAGE_B.to_string(),
                title: "Child".to_string(),
                parent: page_parent(PAGE_A),
                folder_id: None,
                project_id: None,
                first_block_id: BLOCK_B.to_string(),
                after_block_id: None,
                properties: None,
            },
        )
        .await
        .unwrap();
        writes::create_page(
            &pool,
            NotePageCreate {
                id: PAGE_C.to_string(),
                title: "Leaf".to_string(),
                parent: page_parent(PAGE_B),
                folder_id: None,
                project_id: None,
                first_block_id: BLOCK_C.to_string(),
                after_block_id: None,
                properties: None,
            },
        )
        .await
        .unwrap();

        let breadcrumb = reads::page_breadcrumb(&pool, PAGE_C).await.unwrap();
        let breadcrumb_json = serde_json::to_value(breadcrumb).unwrap();
        assert_eq!(breadcrumb_json.as_array().unwrap().len(), 3);
        assert_eq!(breadcrumb_json[0]["id"], PAGE_A);
        assert_eq!(breadcrumb_json[0]["status"], "active");
        assert_eq!(breadcrumb_json[1]["id"], PAGE_B);
        assert_eq!(breadcrumb_json[1]["status"], "active");
        assert_eq!(breadcrumb_json[2]["id"], PAGE_C);
        assert_eq!(breadcrumb_json[2]["current"], true);
    });
}

#[test]
fn page_breadcrumb_marks_unavailable_ancestors() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        writes::create_page(
            &pool,
            NotePageCreate {
                id: PAGE_A.to_string(),
                title: "Archived root".to_string(),
                parent: workspace_parent(),
                folder_id: None,
                project_id: None,
                first_block_id: BLOCK_A.to_string(),
                after_block_id: None,
                properties: None,
            },
        )
        .await
        .unwrap();
        writes::create_page(
            &pool,
            NotePageCreate {
                id: PAGE_B.to_string(),
                title: "Trashed child".to_string(),
                parent: page_parent(PAGE_A),
                folder_id: None,
                project_id: None,
                first_block_id: BLOCK_B.to_string(),
                after_block_id: None,
                properties: None,
            },
        )
        .await
        .unwrap();
        writes::create_page(
            &pool,
            NotePageCreate {
                id: PAGE_C.to_string(),
                title: "Leaf".to_string(),
                parent: page_parent(PAGE_B),
                folder_id: None,
                project_id: None,
                first_block_id: BLOCK_C.to_string(),
                after_block_id: None,
                properties: None,
            },
        )
        .await
        .unwrap();
        sqlx::query("UPDATE notes_pages SET archived = 1 WHERE id = ?")
            .bind(PAGE_A)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE notes_pages SET in_trash = 1 WHERE id = ?")
            .bind(PAGE_B)
            .execute(&pool)
            .await
            .unwrap();

        let breadcrumb = reads::page_breadcrumb(&pool, PAGE_C).await.unwrap();
        let breadcrumb_json = serde_json::to_value(breadcrumb).unwrap();
        assert_eq!(breadcrumb_json[0]["id"], PAGE_A);
        assert_eq!(breadcrumb_json[0]["status"], "archived");
        assert_eq!(breadcrumb_json[1]["id"], PAGE_B);
        assert_eq!(breadcrumb_json[1]["status"], "trashed");
        assert_eq!(breadcrumb_json[2]["id"], PAGE_C);
        assert_eq!(breadcrumb_json[2]["status"], "active");
    });
}

#[test]
fn page_breadcrumb_marks_missing_ancestor_rows() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        sqlx::raw_sql("PRAGMA foreign_keys=OFF")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE notes_pages
             SET parent_type = 'page_id', parent_page_id = ?, child_block_id = id
             WHERE id = ?",
        )
        .bind(PAGE_B)
        .bind(PAGE_A)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql("PRAGMA foreign_keys=ON")
            .execute(&pool)
            .await
            .unwrap();

        let breadcrumb = reads::page_breadcrumb(&pool, PAGE_A).await.unwrap();
        let breadcrumb_json = serde_json::to_value(breadcrumb).unwrap();
        assert_eq!(breadcrumb_json.as_array().unwrap().len(), 2);
        assert_eq!(breadcrumb_json[0]["id"], PAGE_B);
        assert_eq!(breadcrumb_json[0]["status"], "missing");
        assert_eq!(breadcrumb_json[1]["id"], PAGE_A);
        assert_eq!(breadcrumb_json[1]["current"], true);
    });
}

#[test]
fn page_location_follows_project_folder_and_page_ancestry_through_the_page_itself() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        let folder_id = "00000000-0000-4000-8000-000000000301";
        sqlx::query(
            "INSERT INTO project_groups (id, name, order_key) VALUES ('group', 'Group', 'a1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO projects (id, group_id, name, order_key) VALUES ('project', 'group', 'Project', 'a1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_folders (id, project_id, name) VALUES (?, 'project', 'Folder')",
        )
        .bind(folder_id)
        .execute(&pool)
        .await
        .unwrap();
        for (id, title, parent, first_block) in [
            (PAGE_A, "Root", workspace_parent(), BLOCK_A),
            (PAGE_B, "", page_parent(PAGE_A), BLOCK_B),
            (PAGE_C, "Leaf", page_parent(PAGE_B), BLOCK_C),
        ] {
            writes::create_page(
                &pool,
                NotePageCreate {
                    id: id.to_string(),
                    title: title.to_string(),
                    parent,
                    folder_id: None,
                    project_id: None,
                    first_block_id: first_block.to_string(),
                    after_block_id: None,
                    properties: None,
                },
            )
            .await
            .unwrap();
        }
        sqlx::query("UPDATE notes_pages SET folder_id = ?, project_id = 'project' WHERE id = ?")
            .bind(folder_id)
            .bind(PAGE_A)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(
            reads::page_location(&pool, PAGE_C).await.unwrap(),
            ["Group", "Project", "Folder", "Root", "", "Leaf"]
        );
        assert_eq!(
            reads::page_location(&pool, PAGE_A).await.unwrap(),
            ["Group", "Project", "Folder", "Root"]
        );
        sqlx::query("UPDATE notes_pages SET in_trash = 1 WHERE id = ?")
            .bind(PAGE_C)
            .execute(&pool)
            .await
            .unwrap();
        assert!(reads::page_location(&pool, PAGE_C).await.is_err());
        assert!(reads::page_location(&pool, "not-a-page").await.is_err());
    });
}

#[test]
fn page_project_group_lists_the_page_project_and_its_active_peers() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        for (id, name) in [("group", "Group"), ("other-group", "Other")] {
            sqlx::query("INSERT INTO project_groups (id, name, order_key) VALUES (?, ?, 'a1')")
                .bind(id)
                .bind(name)
                .execute(&pool)
                .await
                .unwrap();
        }
        for (id, group, status, order_key) in [
            ("own", "group", "hidden", "a2"),
            ("peer", "group", "active", "a1"),
            ("archived-peer", "group", "archived", "a0"),
            ("elsewhere", "other-group", "active", "a0"),
        ] {
            sqlx::query(
                "INSERT INTO projects (id, group_id, name, status, order_key) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(group)
            .bind(id)
            .bind(status)
            .bind(order_key)
            .execute(&pool)
            .await
            .unwrap();
        }
        for (id, parent, first_block) in [
            (PAGE_A, workspace_parent(), BLOCK_A),
            (PAGE_B, page_parent(PAGE_A), BLOCK_B),
            (PAGE_C, workspace_parent(), BLOCK_C),
        ] {
            writes::create_page(
                &pool,
                NotePageCreate {
                    id: id.to_string(),
                    title: "Page".to_string(),
                    parent,
                    folder_id: None,
                    project_id: None,
                    first_block_id: first_block.to_string(),
                    after_block_id: None,
                    properties: None,
                },
            )
            .await
            .unwrap();
        }
        sqlx::query("UPDATE notes_pages SET project_id = 'own' WHERE id = ?")
            .bind(PAGE_A)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(
            reads::page_project_group_project_ids(&pool, PAGE_B)
                .await
                .unwrap(),
            ["peer", "own"]
        );
        assert!(
            reads::page_project_group_project_ids(&pool, PAGE_C)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            reads::page_project_group_project_ids(&pool, "not-a-page")
                .await
                .is_err()
        );
    });
}
