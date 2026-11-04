use super::helpers::*;

#[test]
fn notes_rich_text_links_validate_local_page_and_block_identities() {
    for url in [
        format!("#notes?page={PAGE_B}"),
        format!("#notes?page={PAGE_B}&block={BLOCK_B}"),
        format!("#notes?block={BLOCK_B}&page={PAGE_B}"),
    ] {
        assert!(
            validation::validate_block_payload(
                "paragraph",
                &json!({ "rich_text": [linked_rich_text("Target", &url)] }),
            )
            .is_ok(),
            "valid local link rejected: {url}"
        );
    }
    for url in [
        "#notes?page=bad".to_string(),
        format!("#notes?block={BLOCK_B}"),
        format!("#notes?page={PAGE_B}&block="),
        format!("#notes?page={PAGE_B}&block=bad"),
        format!("#notes?page={PAGE_B}#other"),
        format!("#notes?page={PAGE_B}\n"),
        format!("#other?page={PAGE_B}"),
        format!("notes:?page={PAGE_B}"),
        "file:///tmp/private".to_string(),
    ] {
        assert!(
            validation::validate_block_payload(
                "paragraph",
                &json!({ "rich_text": [linked_rich_text("Target", &url)] }),
            )
            .is_err(),
            "invalid local link accepted: {url}"
        );
    }
}

#[test]
fn notes_validation_rejects_bad_ids_and_payloads() {
    assert_eq!(
        validation::require_uuid("bad", "id"),
        Err("id must be a UUID".to_string())
    );
    assert_eq!(
        validation::validate_parent(&NoteParent::Workspace { workspace: false }),
        Err("workspace parent must set workspace to true".to_string())
    );
    assert_eq!(
        validation::validate_block_payload("paragraph", &json!({ "rich_text": [{}] })),
        Err("rich text item type must be a string".to_string())
    );
    assert!(
        validation::validate_block_payload(
            "paragraph",
            &json!({ "rich_text": [page_mention(PAGE_B, "Target page")], "color": "default" }),
        )
        .is_ok()
    );
    assert!(
        validation::validate_block_payload("paragraph", &paragraph_icon_payload("Overview"),)
            .is_ok()
    );
    assert!(validation::validate_block_payload(
        "paragraph",
        &json!({ "rich_text": [linked_rich_text("Docs", "https://example.com/docs")], "color": "default" }),
    )
    .is_ok());
    assert!(validation::validate_block_payload(
        "paragraph",
        &json!({ "rich_text": [linked_rich_text("Email", "mailto:team@example.com")], "color": "default" }),
    )
    .is_ok());
    assert!(validation::validate_block_payload(
        "paragraph",
        &json!({ "rich_text": [annotated_rich_text("Important", "blue_background")], "color": "default" }),
    )
    .is_ok());
    assert_eq!(
        validation::validate_block_payload(
            "paragraph",
            &json!({ "rich_text": [annotated_rich_text("Bad", "rainbow")], "color": "default" }),
        ),
        Err("rich text annotations.color must be a supported Notion color".to_string())
    );
    assert!(validation::validate_block_payload(
        "paragraph",
        &json!({ "rich_text": [inline_equation_rich_text("\\frac{a}{b}")], "color": "default" }),
    )
    .is_ok());
    assert!(
        validation::validate_block_payload(
            "heading_1",
            &heading_payload("Toggle heading", true, Some(false)),
        )
        .is_ok()
    );
    assert!(
        validation::validate_block_payload(
            "heading_4",
            &heading_payload("Small toggle heading", true, Some(false)),
        )
        .is_ok()
    );
    assert_eq!(
        validation::validate_block_payload(
            "heading_1",
            &json!({
                "rich_text": [rich_text("Bad heading")],
                "color": "default",
                "is_toggleable": "yes"
            }),
        ),
        Err("heading_1.is_toggleable must be a boolean".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "paragraph",
            &json!({ "rich_text": [inline_equation_rich_text("bad\u{0008}")], "color": "default" }),
        ),
        Err(
            "rich text equation.expression must not be empty or contain control characters"
                .to_string()
        )
    );
    assert_eq!(
        validation::validate_block_payload(
            "paragraph",
            &json!({ "rich_text": [linked_rich_text("Bad", "javascript:alert(1)")], "color": "default" }),
        ),
        Err("rich text text.link.url must be a valid HTTP, HTTPS, or email URL".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "paragraph",
            &json!({ "rich_text": [linked_rich_text("Bad", "mailto:team@example")], "color": "default" }),
        ),
        Err("rich text text.link.url must be a valid HTTP, HTTPS, or email URL".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "paragraph",
            &json!({ "rich_text": [linked_rich_text("Bad", "mailto:team%40example.com")], "color": "default" }),
        ),
        Err("rich text text.link.url must be a valid HTTP, HTTPS, or email URL".to_string())
    );
    assert!(validation::validate_block_payload(
        "paragraph",
        &json!({ "rich_text": [date_mention("2026-06-30", "Remind Today", true)], "color": "default" }),
    )
    .is_ok());
    assert_eq!(
        validation::validate_block_payload(
            "paragraph",
            &json!({ "rich_text": [date_mention("2026-99-30", "Bad date", false)], "color": "default" }),
        ),
        Err("rich text mention.date.start must be an ISO date or date-time".to_string())
    );
    let formatted = |format: serde_json::Value| {
        let mut mention = date_mention("2026-10-30T12:01", "October 30, 2026, 12:01 PM", true);
        mention["mention"]["date"]["ganbaru_format"] = format;
        json!({ "rich_text": [mention], "color": "default" })
    };
    for format in ["full", "short", "numeric", "iso"] {
        assert!(
            validation::validate_block_payload("paragraph", &formatted(json!(format))).is_ok(),
            "{format}"
        );
    }
    assert!(validation::validate_block_payload("paragraph", &formatted(json!(null))).is_ok());
    assert_eq!(
        validation::validate_block_payload("paragraph", &formatted(json!("relative"))),
        Err("rich text mention.date.ganbaru_format must be null or one of full, short, numeric, iso".to_string())
    );
    let offset = |minutes: serde_json::Value| {
        let mut mention = date_mention("2026-10-30T12:01", "Remind October 30, 2026", true);
        mention["mention"]["date"]["ganbaru_reminder"]["minutes_before"] = minutes;
        json!({ "rich_text": [mention], "color": "default" })
    };
    for minutes in [json!(null), json!(0), json!(30), json!(40_320)] {
        assert!(
            validation::validate_block_payload("paragraph", &offset(minutes.clone())).is_ok(),
            "{minutes}"
        );
    }
    for minutes in [json!(-5), json!(40_321), json!(2.5), json!("30")] {
        assert_eq!(
            validation::validate_block_payload("paragraph", &offset(minutes)),
            Err("rich text mention.date.ganbaru_reminder.minutes_before must be null or a whole number from 0 to 40320".to_string())
        );
    }
    assert_eq!(
        validation::validate_block_payload(
            "paragraph",
            &json!({
                "rich_text": [{
                    "type": "mention",
                    "mention": {
                        "type": "database",
                        "database": { "id": PAGE_B }
                    },
                    "annotations": {
                        "bold": false,
                        "italic": false,
                        "strikethrough": false,
                        "underline": false,
                        "code": false,
                        "color": "default"
                    },
                    "plain_text": "Target database",
                    "href": null
                }],
                "color": "default"
            }),
        ),
        Ok(())
    );
    assert_eq!(
        validation::validate_block_payload(
            "paragraph",
            &json!({
                "rich_text": [{
                    "type": "mention",
                    "mention": {
                        "type": "ganbaru_object",
                        "ganbaru_object": {
                            "type": "project_task",
                            "id": PAGE_B
                        }
                    },
                    "annotations": {
                        "bold": false,
                        "italic": false,
                        "strikethrough": false,
                        "underline": false,
                        "code": false,
                        "color": "default"
                    },
                    "plain_text": "Target task",
                    "href": null
                }],
                "color": "default"
            }),
        ),
        Ok(())
    );
    assert_eq!(
        validation::validate_block_payload(
            "paragraph",
            &json!({
                "rich_text": [{
                    "type": "mention",
                    "mention": {
                        "type": "link_preview",
                        "link_preview": { "url": "https://example.com" }
                    },
                    "annotations": {
                        "bold": false,
                        "italic": false,
                        "strikethrough": false,
                        "underline": false,
                        "code": false,
                        "color": "default"
                    },
                    "plain_text": "Example",
                    "href": null
                }],
                "color": "default"
            }),
        ),
        Err("rich text mention.type is unsupported".to_string())
    );
    assert_eq!(
        validation::validate_block_payload("nope", &json!({})),
        Err("unsupported block type: nope".to_string())
    );
    for block_type in ["meeting_notes", "transcription"] {
        assert_eq!(
            validation::validate_block_type(block_type),
            Err(format!(
                "{block_type} is blocked by the Notes block catalog gate until rich editor P0 completion, current block quality completion, honest docs, usable editing UI, persistence, focused tests, pnpm -w run validate are complete"
            ))
        );
        assert_eq!(
            validation::validate_block_payload(block_type, &json!({})),
            Err(format!(
                "{block_type} is blocked by the Notes block catalog gate until rich editor P0 completion, current block quality completion, honest docs, usable editing UI, persistence, focused tests, pnpm -w run validate are complete"
            ))
        );
    }
    assert_eq!(
        validation::validate_block_payload(
            "paragraph",
            &json!({ "rich_text": [rich_text("Bad")], "color": "neon" }),
        ),
        Err("paragraph.color must be a supported Notion color".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "toggle",
            &json!({ "rich_text": [rich_text("Bad")], "color": "default", "ganbaru_open": "yes" }),
        ),
        Err("toggle.ganbaru_open must be a boolean".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "callout",
            &json!({
                "rich_text": [rich_text("Bad")],
                "color": "default",
                "icon": { "type": "icon", "icon": { "name": "" } }
            }),
        ),
        Err("callout.icon.icon.name must be a non-empty string".to_string())
    );
    assert_eq!(
        validation::validate_block_payload("table_of_contents", &json!({ "color": "neon" })),
        Err("table_of_contents.color must be a supported Notion color".to_string())
    );
    assert_eq!(
        validation::validate_block_payload("child_page", &json!({ "title": 42 })),
        Err("child_page.title must be a string".to_string())
    );
    assert_eq!(
        validation::validate_block_payload("child_page", &json!({ "title": "bad\u{0008}" })),
        Err("child_page.title must not contain control characters".to_string())
    );
    assert!(
        validation::validate_block_payload(
            "child_database",
            &json!({
                "title": "Tasks",
                "database_id": DATABASE_A,
                "data_source_id": DATA_SOURCE_A,
                "view_id": DATABASE_VIEW_A
            }),
        )
        .is_ok()
    );
    assert_eq!(
        validation::validate_block_payload(
            "child_database",
            &json!({ "title": "Tasks", "database_id": "not-a-uuid" }),
        ),
        Err("child_database.database_id must be a UUID".to_string())
    );
    assert!(validation::validate_block_payload("template", &template_payload("Add task")).is_ok());
    assert_eq!(
        validation::validate_block_payload("template", &json!({ "rich_text": "Add task" })),
        Err("template.rich_text must be an array".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "template",
            &json!({ "rich_text": [rich_text("Add task")], "color": "default" }),
        ),
        Err("template.color is not supported".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "template",
            &json!({ "rich_text": [rich_text("Add task")], "children": [] }),
        ),
        Err("template.children must be stored as child blocks".to_string())
    );
    assert!(validation::validate_block_payload("button", &button_payload("Add agenda")).is_ok());
    for position in [
        "below_button",
        "above_button",
        "top_of_page",
        "bottom_of_page",
    ] {
        assert!(
            validation::validate_block_payload(
                "button",
                &json!({
                    "rich_text": [rich_text("Add agenda")],
                    "icon": null,
                    "actions": [{
                        "type": "insert_blocks",
                        "source": "children",
                        "position": position
                    }]
                }),
            )
            .is_ok()
        );
    }
    assert_eq!(
        validation::validate_block_payload(
            "button",
            &json!({
                "rich_text": [rich_text("Add agenda")],
                "icon": null,
                "actions": []
            }),
        ),
        Err("button.actions must include between 1 and 10 actions".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "button",
            &json!({
                "rich_text": [rich_text("Add agenda")],
                "icon": null,
                "actions": [{
                    "type": "send_webhook",
                    "source": "children",
                    "position": "below_button"
                }]
            }),
        ),
        Err("button.actions[0].type must be insert_blocks".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "button",
            &json!({
                "rich_text": [rich_text("Add agenda")],
                "icon": null,
                "actions": [{
                    "type": "insert_blocks",
                    "source": "children",
                    "position": "nearby_database"
                }]
            }),
        ),
        Err("button.actions[0].position must be a supported button insert position".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "button",
            &json!({
                "rich_text": [rich_text("Add agenda")],
                "actions": [{
                    "type": "insert_blocks",
                    "source": "children",
                    "position": "below_button"
                }]
            }),
        ),
        Err("button.icon is required".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "button",
            &json!({
                "rich_text": [rich_text("Add agenda")],
                "icon": null,
                "children": [],
                "actions": [{
                    "type": "insert_blocks",
                    "source": "children",
                    "position": "below_button"
                }]
            }),
        ),
        Err("button.children must be stored as child blocks".to_string())
    );
    assert_eq!(
        validation::validate_block_payload("child_database", &json!({ "title": 42 })),
        Err("child_database.title must be a string".to_string())
    );
    assert_eq!(
        validation::validate_block_payload("child_database", &json!({ "title": "bad\u{0008}" }),),
        Err("child_database.title must not contain control characters".to_string())
    );
    assert_eq!(
        validation::validate_block_payload("column", &json!({ "width_ratio": 0.0 })),
        Err("column.width_ratio must be greater than 0 and no more than 1".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "table",
            &json!({
                "table_width": 0,
                "has_column_header": false,
                "has_row_header": false
            }),
        ),
        Err("table.table_width must be between 1 and 100".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "table",
            &json!({
                "table_width": 2,
                "has_column_header": "yes",
                "has_row_header": false
            }),
        ),
        Err("table.has_column_header must be a boolean".to_string())
    );
    assert_eq!(
        validation::validate_block_payload("table_row", &json!({ "cells": [{}] })),
        Err("table_row.cells entries must be rich text arrays".to_string())
    );
    assert!(validation::validate_block_payload("tab", &tab_payload()).is_ok());
    assert_eq!(
        validation::validate_block_payload("tab", &json!({ "title": "Bad" })),
        Err("tab must be an empty object".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "bookmark",
            &json!({ "caption": [{}], "url": "https://example.com" }),
        ),
        Err("rich text item type must be a string".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "bookmark",
            &json!({ "caption": [], "url": "bad\u{0008}url" }),
        ),
        Err(
            "bookmark.url must not contain control characters or a javascript, data, vbscript, or file scheme"
                .to_string()
        )
    );
    assert_eq!(
        validation::validate_block_payload("equation", &json!({ "expression": 42 })),
        Err("equation.expression must be a string".to_string())
    );
    assert_eq!(
        validation::validate_block_payload("embed", &json!({ "url": 42 })),
        Err("embed.url must be a string".to_string())
    );
    assert_eq!(
        validation::validate_block_payload("embed", &json!({ "url": "bad\u{0008}url" })),
        Err("embed.url must be an http or https URL without control characters".to_string())
    );
    assert_eq!(
        validation::validate_block_payload("link_preview", &json!({ "url": 42 })),
        Err("link_preview.url must be a string".to_string())
    );
    assert_eq!(
        validation::validate_block_payload("link_preview", &json!({ "url": "bad\u{0008}url" }),),
        Err("link_preview.url must be an http or https URL without control characters".to_string())
    );
    assert!(
        validation::validate_block_payload("synced_block", &synced_block_payload_original())
            .is_ok()
    );
    assert!(
        validation::validate_block_payload(
            "synced_block",
            &synced_block_payload_duplicate(BLOCK_A),
        )
        .is_ok()
    );
    assert_eq!(
        validation::validate_block_payload("synced_block", &json!({})),
        Err("synced_block.synced_from is required".to_string())
    );
    assert_eq!(
        validation::validate_block_payload("synced_block", &json!({ "synced_from": "bad" })),
        Err("synced_block.synced_from must be null or an object".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "synced_block",
            &json!({ "synced_from": { "type": "page_id", "block_id": BLOCK_A } }),
        ),
        Err("synced_block.synced_from.type must be block_id".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "synced_block",
            &json!({ "synced_from": { "type": "block_id", "block_id": "bad" } }),
        ),
        Err("synced_block.synced_from.block_id must be a UUID".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "image",
            &media_payload("http://example.com/image.png", "", None),
        ),
        Err("image.external.url must be a supported HTTPS image URL".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "pdf",
            &media_payload("https://example.com/file.txt", "", None),
        ),
        Err("pdf.external.url must be a supported HTTPS pdf URL".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "file",
            &json!({
                "caption": [],
                "type": "file_upload",
                "file_upload": { "id": "bad" }
            }),
        ),
        Err("file.file_upload.id must be a UUID".to_string())
    );
    assert!(
        validation::validate_block_payload(
            "image",
            &local_media_payload(
                "notes/files/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
                "image/png",
                42,
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "",
                Some("image.png"),
            ),
        )
        .is_ok()
    );
    assert_eq!(
        validation::validate_block_payload(
            "image",
            &local_media_payload(
                "notes/page-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
                "image/png",
                42,
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "",
                Some("image.png"),
            ),
        ),
        Err(
            "image.file.ganbaru_asset_path must stay under the managed Notes file directory"
                .to_string()
        )
    );
    assert_eq!(
        validation::validate_block_payload(
            "pdf",
            &local_media_payload(
                "notes/files/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.pdf",
                "text/plain",
                42,
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "",
                Some("report.pdf"),
            ),
        ),
        Err("pdf.file.content_type must match the local media block type".to_string())
    );
    assert_eq!(
        validation::validate_block_payload("equation", &json!({ "expression": "bad\u{0008}" })),
        Err("equation.expression must not contain control characters".to_string())
    );
    assert_eq!(
        validation::validate_block_payload("unsupported", &json!({ "block_type": "" })),
        Err("unsupported.block_type must not be empty".to_string())
    );
    assert_eq!(
        validation::validate_block_payload(
            "unsupported",
            &json!({ "block_type": "bad\u{0008}type" }),
        ),
        Err("unsupported.block_type must not contain control characters".to_string())
    );
    assert_eq!(
        validation::validate_block_payload("unsupported", &json!({ "raw": "form" })),
        Err("unsupported.raw must be an object".to_string())
    );
    assert_eq!(
        validation::validate_block_payload("unsupported", &json!({ "warnings": [""] })),
        Err("unsupported.warnings[0] must not be empty".to_string())
    );
}

#[test]
fn notes_validation_boundary_matrix_keeps_acceptance_and_errors_stable() {
    let managed_image = local_media_payload(
        "notes/files/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
        "image/png",
        42,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "",
        Some("image.png"),
    );
    let escaped_image = local_media_payload(
        "notes/page-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
        "image/png",
        42,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "",
        Some("image.png"),
    );
    let cases = [
        (
            "managed Notes file",
            validation::validate_block_payload("image", &managed_image),
            Ok(()),
        ),
        (
            "managed path outside Notes files",
            validation::validate_block_payload("image", &escaped_image),
            Err("image.file.ganbaru_asset_path must stay under the managed Notes file directory"
                .to_string()),
        ),
        (
            "HTTPS rich-text URL",
            validation::validate_block_payload(
                "paragraph",
                &json!({
                    "rich_text": [linked_rich_text("Docs", "https://example.com/docs")],
                    "color": "default"
                }),
            ),
            Ok(()),
        ),
        (
            "blocked rich-text URL scheme",
            validation::validate_block_payload(
                "paragraph",
                &json!({
                    "rich_text": [linked_rich_text("Bad", "javascript:alert(1)")],
                    "color": "default"
                }),
            ),
            Err("rich text text.link.url must be a valid HTTP, HTTPS, or email URL".to_string()),
        ),
        (
            "control character",
            validation::validate_block_payload(
                "equation",
                &json!({ "expression": "bad\u{0008}" }),
            ),
            Err("equation.expression must not contain control characters".to_string()),
        ),
        (
            "ISO date mention",
            validation::validate_block_payload(
                "paragraph",
                &json!({
                    "rich_text": [date_mention("2026-06-30", "Today", true)],
                    "color": "default"
                }),
            ),
            Ok(()),
        ),
        (
            "invalid date mention",
            validation::validate_block_payload(
                "paragraph",
                &json!({
                    "rich_text": [date_mention("2026-99-30", "Bad date", false)],
                    "color": "default"
                }),
            ),
            Err("rich text mention.date.start must be an ISO date or date-time".to_string()),
        ),
        (
            "blocked future block",
            validation::validate_block_type("meeting_notes"),
            Err("meeting_notes is blocked by the Notes block catalog gate until rich editor P0 completion, current block quality completion, honest docs, usable editing UI, persistence, focused tests, pnpm -w run validate are complete".to_string()),
        ),
    ];

    for (label, actual, expected) in cases {
        assert_eq!(actual, expected, "{label}");
    }
}

#[test]
fn notes_validation_accepts_readable_app_object_mention_ids() {
    let object_mention = |object_type: &str, id: &str| {
        json!({
            "rich_text": [{
                "type": "mention",
                "mention": {
                    "type": "ganbaru_object",
                    "ganbaru_object": { "type": object_type, "id": id }
                },
                "annotations": {
                    "bold": false,
                    "italic": false,
                    "strikethrough": false,
                    "underline": false,
                    "code": false,
                    "color": "default"
                },
                "plain_text": "Chores",
                "href": null
            }],
            "color": "default"
        })
    };

    assert_eq!(
        validation::validate_block_payload(
            "paragraph",
            &object_mention("project", "project-routine-chores")
        ),
        Ok(())
    );
    assert_eq!(
        validation::validate_block_payload("paragraph", &object_mention("project_task", " ")),
        Err(
            "rich text mention.ganbaru_object.id must not be empty or contain control characters"
                .to_string()
        )
    );
    for live_state in ["pomodoro_run", "music_item"] {
        assert_eq!(
            validation::validate_block_payload("paragraph", &object_mention(live_state, "item-1")),
            Err("rich text mention.ganbaru_object.type is unsupported".to_string()),
            "{live_state}"
        );
    }
}

#[test]
fn web_reference_blocks_reject_script_and_local_data_schemes() {
    for url in [
        "javascript:alert(1)",
        " JavaScript:alert(1)",
        "data:text/html,<script>alert(1)</script>",
        "vbscript:msgbox(1)",
        "file:///etc/passwd",
        "javascript:1",
    ] {
        for block_type in ["bookmark", "embed", "link_preview"] {
            let payload = if block_type == "bookmark" {
                json!({ "caption": [], "url": url })
            } else {
                json!({ "url": url })
            };
            assert!(
                validation::validate_block_payload(block_type, &payload).is_err(),
                "{block_type} accepted {url}"
            );
        }
    }
}

#[test]
fn bookmarks_keep_email_phone_app_links_and_drafts() {
    for url in [
        "",
        "https://example.com",
        "mailto:me@example.com",
        "tel:+15550100",
        "obsidian://open?vault=Work",
        "zotero://select/items/1",
        "javascript",
        "example.com/a:b",
    ] {
        assert!(
            validation::validate_block_payload("bookmark", &json!({ "caption": [], "url": url }))
                .is_ok(),
            "bookmark rejected {url}"
        );
    }
}

#[test]
fn embeds_and_link_previews_keep_web_urls_and_typing_drafts_only() {
    for block_type in ["embed", "link_preview"] {
        for url in [
            "",
            "https://example.com/video",
            "HTTP://example.com",
            "http:",
            "example.com/watch",
            "localhost:3000",
            "www.example.com:8080/path",
        ] {
            assert!(
                validation::validate_block_payload(block_type, &json!({ "url": url })).is_ok(),
                "{block_type} rejected {url}"
            );
        }
        for url in [
            "mailto:me@example.com",
            "obsidian://open",
            "ftp://example.com",
        ] {
            assert!(
                validation::validate_block_payload(block_type, &json!({ "url": url })).is_err(),
                "{block_type} accepted {url}"
            );
        }
    }
}

#[test]
fn exported_web_references_link_only_explicit_safe_schemes() {
    assert!(validation::is_exportable_web_reference_url(
        "bookmark",
        "tel:+15550100"
    ));
    assert!(validation::is_exportable_web_reference_url(
        "bookmark",
        "obsidian://open"
    ));
    assert!(validation::is_exportable_web_reference_url(
        "embed",
        "https://example.com"
    ));
    assert!(!validation::is_exportable_web_reference_url(
        "embed",
        "mailto:me@example.com"
    ));
    assert!(!validation::is_exportable_web_reference_url(
        "bookmark",
        "java\tscript:alert(1)"
    ));
    assert!(!validation::is_exportable_web_reference_url(
        "bookmark",
        "../notes.md"
    ));
    assert!(!validation::is_exportable_web_reference_url("bookmark", ""));
    assert_eq!(
        validation::uri_scheme(" Java\nScript:x").as_deref(),
        Some("javascript")
    );
    assert_eq!(validation::uri_scheme("example.com/a:b"), None);
}
