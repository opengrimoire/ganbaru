pub(super) use crate::models::{
    NoteAgentBridgeExportRequest, NoteAppendBlockChildren, NoteBlockHydrationRequest,
    NoteBlockUpdate, NoteBlockWrite, NoteChildPageFromBlockCreate, NoteCommentAnchorCreate,
    NoteCommentCreate, NoteCommentThreadReadUpdate, NoteCommentUpdate,
    NoteDataSourceBoardConfigurationUpdate, NoteDataSourceBoardRowMove,
    NoteDataSourceBoardViewUpdate, NoteDataSourceButtonClick,
    NoteDataSourceCalendarConfigurationUpdate, NoteDataSourceCalendarViewUpdate,
    NoteDataSourceCsvExportRequest, NoteDataSourceCsvImportRequest,
    NoteDataSourceGalleryConfigurationUpdate, NoteDataSourceGalleryViewUpdate,
    NoteDataSourceListConfigurationUpdate, NoteDataSourceListViewUpdate,
    NoteDataSourceRowPageCreate, NoteDataSourceRowPropertyUpdate, NoteDataSourceSchemaUpdate,
    NoteDataSourceTableConfigurationUpdate, NoteDataSourceTableFilter, NoteDataSourceTableSort,
    NoteDataSourceTableViewUpdate, NoteDataSourceTemplateApply,
    NoteDataSourceTemplateCreateFromRow, NoteDataSourceTimelineConfigurationUpdate,
    NoteDataSourceTimelineViewUpdate, NoteDataSourceViewWindowRequest, NoteDatabaseCreate,
    NoteDuplicateBlock, NoteDuplicateBlocks, NoteDuplicatePage, NoteDuplicatedBlockId,
    NoteFolderCreate, NoteFolderUpdate, NoteHtmlExportRequest, NoteHtmlImportRequest,
    NoteJsonGraphExportRequest, NoteLinkedDatabaseCreate, NoteLocalUserUpdate,
    NoteMarkdownExportRequest, NoteMarkdownImportRequest, NoteMentionNotificationDeliveryUpdate,
    NoteMoveBlock, NoteMoveBlocks, NoteMovePage, NoteNotionExportImportRequest,
    NotePageAliasCreate, NotePageCreate, NotePageHistoryCopyBlocks, NotePageHistorySettingsUpdate,
    NotePageTemplateApply, NotePageTemplateCreateFromPage, NotePageTemplateDuplicate,
    NotePageTemplateUpdate, NotePageUpdate, NoteParent, NoteSidebarPagesRequest,
    NoteSuggestionCreate, NoteTrashBlocks, NoteUnresolvedLinkResolve, NoteWorkspaceShellRequest,
    OptionalJsonValue,
};
pub(super) use crate::{
    assets, comments, data_sources, databases, folders, links, local_user, mention_notifications,
    page_history, project_history, reads, search, suggestions, templates, transfers, undo_state,
    validation, workspace_shell, writes,
};
pub(super) use ganbaru_db::run_migrations;
pub(super) use serde_json::json;
pub(super) use sqlx::{Row, SqlitePool};

pub(super) const PAGE_A: &str = "11111111-1111-4111-8111-111111111111";
pub(super) const PAGE_B: &str = "22222222-2222-4222-8222-222222222222";
pub(super) const PAGE_C: &str = "33333333-3333-4333-8333-333333333333";
pub(super) const BLOCK_A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
pub(super) const BLOCK_B: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
pub(super) const BLOCK_C: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
pub(super) const BLOCK_D: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
pub(super) const BLOCK_E: &str = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";
pub(super) const BLOCK_F: &str = "ffffffff-ffff-4fff-8fff-ffffffffffff";
pub(super) const COMMENT_A: &str = "10101010-1010-4010-8010-101010101010";
pub(super) const COMMENT_B: &str = "20202020-2020-4020-8020-202020202020";
pub(super) const COMMENT_C: &str = "30303030-3030-4030-8030-303030303030";
pub(super) const SUGGESTION_A: &str = "40404040-4040-4040-8040-404040404040";
pub(super) const SUGGESTION_B: &str = "50505050-5050-4050-8050-505050505050";
pub(super) const TEMPLATE_A: &str = "90909090-9090-4090-8090-909090909090";
pub(super) const TEMPLATE_B: &str = "91919191-9191-4191-8191-919191919191";
pub(super) const DATABASE_A: &str = "80808080-8080-4080-8080-808080808080";
pub(super) const DATA_SOURCE_A: &str = "81818181-8181-4181-8181-818181818181";
pub(super) const DATABASE_VIEW_A: &str = "82828282-8282-4282-8282-828282828282";
pub(super) const LINKED_DATABASE_A: &str = "83838383-8383-4383-8383-838383838383";
pub(super) const LINKED_DATABASE_VIEW_A: &str = "84848484-8484-4484-8484-848484848484";
pub(super) const DATABASE_B: &str = "85858585-8585-4585-8585-858585858585";
pub(super) const DATA_SOURCE_B: &str = "86868686-8686-4686-8686-868686868686";
pub(super) const DATABASE_VIEW_B: &str = "87878787-8787-4787-8787-878787878787";

pub(super) async fn migrated_memory_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    run_migrations(&pool).await.unwrap();
    pool
}

pub(super) fn workspace_parent() -> NoteParent {
    NoteParent::Workspace { workspace: true }
}

pub(super) fn page_parent(page_id: &str) -> NoteParent {
    NoteParent::PageId {
        page_id: page_id.to_string(),
    }
}

pub(super) fn block_parent(block_id: &str) -> NoteParent {
    NoteParent::BlockId {
        block_id: block_id.to_string(),
    }
}

pub(super) fn rich_text(content: &str) -> serde_json::Value {
    json!({
        "type": "text",
        "text": {
            "content": content,
            "link": null
        },
        "annotations": {
            "bold": false,
            "italic": false,
            "strikethrough": false,
            "underline": false,
            "code": false,
            "color": "default"
        },
        "plain_text": content,
        "href": null
    })
}

pub(super) fn linked_rich_text(content: &str, url: &str) -> serde_json::Value {
    json!({
        "type": "text",
        "text": {
            "content": content,
            "link": {
                "url": url
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
        "plain_text": content,
        "href": url
    })
}

pub(super) fn annotated_rich_text(content: &str, color: &str) -> serde_json::Value {
    json!({
        "type": "text",
        "text": {
            "content": content,
            "link": null
        },
        "annotations": {
            "bold": true,
            "italic": true,
            "strikethrough": false,
            "underline": true,
            "code": false,
            "color": color
        },
        "plain_text": content,
        "href": null
    })
}

pub(super) fn inline_equation_rich_text(expression: &str) -> serde_json::Value {
    json!({
        "type": "equation",
        "equation": {
            "expression": expression
        },
        "annotations": {
            "bold": false,
            "italic": false,
            "strikethrough": false,
            "underline": false,
            "code": false,
            "color": "default"
        },
        "plain_text": expression,
        "href": null
    })
}

pub(super) fn page_mention(page_id: &str, title: &str) -> serde_json::Value {
    json!({
        "type": "mention",
        "mention": {
            "type": "page",
            "page": {
                "id": page_id
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
        "plain_text": title,
        "href": format!("http://localhost:1420/?view=notes#notes?page={page_id}")
    })
}

pub(super) fn date_mention(start: &str, title: &str, reminder: bool) -> serde_json::Value {
    let mut date = json!({
        "start": start,
        "end": null,
        "time_zone": null
    });
    if reminder {
        date["ganbaru_reminder"] = json!({ "enabled": true });
    }
    json!({
        "type": "mention",
        "mention": {
            "type": "date",
            "date": date
        },
        "annotations": {
            "bold": false,
            "italic": false,
            "strikethrough": false,
            "underline": false,
            "code": false,
            "color": "default"
        },
        "plain_text": title,
        "href": null
    })
}

pub(super) fn user_mention(user_id: &str, title: &str) -> serde_json::Value {
    json!({
        "type": "mention",
        "mention": {
            "type": "user",
            "user": {
                "object": "user",
                "id": user_id
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
        "plain_text": title,
        "href": null
    })
}

pub(super) fn project_task_mention(task_id: &str, title: &str) -> serde_json::Value {
    json!({
        "type": "mention",
        "mention": {
            "type": "ganbaru_object",
            "ganbaru_object": {
                "type": "project_task",
                "id": task_id
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
        "plain_text": title,
        "href": null
    })
}

pub(super) fn paragraph_payload(content: &str) -> serde_json::Value {
    json!({
        "rich_text": [rich_text(content)],
        "color": "default"
    })
}

pub(super) fn paragraph_icon_payload(content: &str) -> serde_json::Value {
    json!({
        "rich_text": [rich_text(content)],
        "color": "default",
        "icon": {
            "type": "icon",
            "icon": {
                "name": "star",
                "color": "yellow"
            }
        }
    })
}

pub(super) fn heading_payload(
    content: &str,
    is_toggleable: bool,
    open: Option<bool>,
) -> serde_json::Value {
    let mut payload = paragraph_payload(content);
    payload["is_toggleable"] = json!(is_toggleable);
    if let Some(open) = open {
        payload["ganbaru_open"] = json!(open);
    }
    payload
}

pub(super) fn todo_payload(content: &str, checked: bool) -> serde_json::Value {
    json!({
        "rich_text": [rich_text(content)],
        "checked": checked,
        "color": "default"
    })
}

pub(super) fn code_payload(content: &str, language: &str) -> serde_json::Value {
    json!({
        "rich_text": [rich_text(content)],
        "caption": [],
        "language": language
    })
}

pub(super) fn bookmark_payload(url: &str, caption: &str) -> serde_json::Value {
    let caption_items = if caption.is_empty() {
        Vec::<serde_json::Value>::new()
    } else {
        vec![rich_text(caption)]
    };
    json!({
        "caption": caption_items,
        "url": url
    })
}

pub(super) fn embed_payload(url: &str) -> serde_json::Value {
    json!({
        "url": url
    })
}

pub(super) fn link_preview_payload(url: &str) -> serde_json::Value {
    json!({
        "url": url
    })
}

pub(super) fn synced_block_payload_original() -> serde_json::Value {
    json!({
        "synced_from": null
    })
}

pub(super) fn synced_block_payload_duplicate(block_id: &str) -> serde_json::Value {
    json!({
        "synced_from": {
            "type": "block_id",
            "block_id": block_id
        }
    })
}

pub(super) fn child_database_payload(title: &str) -> serde_json::Value {
    json!({
        "title": title
    })
}

pub(super) fn template_payload(title: &str) -> serde_json::Value {
    json!({
        "rich_text": [rich_text(title)]
    })
}

pub(super) fn button_payload(title: &str) -> serde_json::Value {
    json!({
        "rich_text": [rich_text(title)],
        "icon": {
            "type": "icon",
            "icon": {
                "name": "mouse-pointer-click",
                "color": "gray"
            }
        },
        "actions": [{
            "type": "insert_blocks",
            "source": "children",
            "position": "below_button"
        }]
    })
}

pub(super) fn equation_payload(expression: &str) -> serde_json::Value {
    json!({
        "expression": expression
    })
}

pub(super) fn unsupported_payload(block_type: &str) -> serde_json::Value {
    json!({
        "block_type": block_type,
        "source_type": "notion",
        "raw": {
            "type": "unsupported",
            "unsupported": {
                "block_type": block_type
            }
        },
        "warnings": ["Content is not exposed by the source API"]
    })
}

pub(super) fn media_payload(url: &str, caption: &str, name: Option<&str>) -> serde_json::Value {
    let caption_items = if caption.is_empty() {
        Vec::<serde_json::Value>::new()
    } else {
        vec![rich_text(caption)]
    };
    let mut payload = json!({
        "caption": caption_items,
        "type": "external",
        "external": {
            "url": url
        }
    });
    if let Some(name) = name {
        payload["name"] = json!(name);
    }
    payload
}

pub(super) fn local_media_payload(
    asset_path: &str,
    content_type: &str,
    byte_size: i64,
    sha256: &str,
    caption: &str,
    name: Option<&str>,
) -> serde_json::Value {
    let caption_items = if caption.is_empty() {
        Vec::<serde_json::Value>::new()
    } else {
        vec![rich_text(caption)]
    };
    let mut payload = json!({
        "caption": caption_items,
        "type": "file",
        "file": {
            "url": format!("ganbaru-asset:{asset_path}"),
            "content_type": content_type,
            "byte_size": byte_size,
            "sha256": sha256,
            "ganbaru_asset_path": asset_path
        }
    });
    if let Some(name) = name {
        payload["name"] = json!(name);
        payload["file"]["name"] = json!(name);
    }
    payload
}

pub(super) fn local_property_file(
    asset_path: &str,
    content_type: &str,
    byte_size: i64,
    sha256: &str,
    name: &str,
) -> serde_json::Value {
    json!({
        "name": name,
        "type": "file",
        "file": {
            "url": format!("ganbaru-asset:{asset_path}"),
            "name": name,
            "content_type": content_type,
            "byte_size": byte_size,
            "sha256": sha256,
            "ganbaru_asset_path": asset_path
        }
    })
}

pub(super) fn local_comment_attachment(
    asset_path: &str,
    content_type: &str,
    byte_size: i64,
    sha256: &str,
    name: &str,
) -> serde_json::Value {
    json!({
        "category": "file",
        "name": name,
        "file": {
            "url": format!("ganbaru-asset:{asset_path}"),
            "name": name,
            "content_type": content_type,
            "byte_size": byte_size,
            "sha256": sha256,
            "ganbaru_asset_path": asset_path
        }
    })
}

pub(super) fn table_payload(width: i64) -> serde_json::Value {
    json!({
        "table_width": width,
        "has_column_header": false,
        "has_row_header": false
    })
}

pub(super) fn column_payload(width_ratio: Option<f64>) -> serde_json::Value {
    match width_ratio {
        Some(width_ratio) => json!({ "width_ratio": width_ratio }),
        None => json!({}),
    }
}

pub(super) fn table_row_payload(cells: &[&str]) -> serde_json::Value {
    json!({
        "cells": cells
            .iter()
            .map(|cell| {
                if cell.is_empty() {
                    Vec::<serde_json::Value>::new()
                } else {
                    vec![rich_text(cell)]
                }
            })
            .collect::<Vec<_>>()
    })
}

pub(super) fn tab_payload() -> serde_json::Value {
    json!({})
}

pub(super) fn block(id: &str, block_type: &str, payload: serde_json::Value) -> NoteBlockWrite {
    NoteBlockWrite {
        id: id.to_string(),
        block_type: block_type.to_string(),
        paragraph: (block_type == "paragraph").then_some(payload.clone()),
        heading_1: (block_type == "heading_1").then_some(payload.clone()),
        heading_2: (block_type == "heading_2").then_some(payload.clone()),
        heading_3: (block_type == "heading_3").then_some(payload.clone()),
        heading_4: (block_type == "heading_4").then_some(payload.clone()),
        heading_5: (block_type == "heading_5").then_some(payload.clone()),
        heading_6: (block_type == "heading_6").then_some(payload.clone()),
        bulleted_list_item: (block_type == "bulleted_list_item").then_some(payload.clone()),
        numbered_list_item: (block_type == "numbered_list_item").then_some(payload.clone()),
        to_do: (block_type == "to_do").then_some(payload.clone()),
        toggle: (block_type == "toggle").then_some(payload.clone()),
        callout: (block_type == "callout").then_some(payload.clone()),
        quote: (block_type == "quote").then_some(payload.clone()),
        child_page: (block_type == "child_page").then_some(payload.clone()),
        child_database: (block_type == "child_database").then_some(payload.clone()),
        breadcrumb: (block_type == "breadcrumb").then_some(payload.clone()),
        table_of_contents: (block_type == "table_of_contents").then_some(payload.clone()),
        column_list: (block_type == "column_list").then_some(payload.clone()),
        column: (block_type == "column").then_some(payload.clone()),
        table: (block_type == "table").then_some(payload.clone()),
        table_row: (block_type == "table_row").then_some(payload.clone()),
        tab: (block_type == "tab").then_some(payload.clone()),
        image: (block_type == "image").then_some(payload.clone()),
        video: (block_type == "video").then_some(payload.clone()),
        audio: (block_type == "audio").then_some(payload.clone()),
        file: (block_type == "file").then_some(payload.clone()),
        pdf: (block_type == "pdf").then_some(payload.clone()),
        bookmark: (block_type == "bookmark").then_some(payload.clone()),
        link_preview: (block_type == "link_preview").then_some(payload.clone()),
        synced_block: (block_type == "synced_block").then_some(payload.clone()),
        template: (block_type == "template").then_some(payload.clone()),
        button: (block_type == "button").then_some(payload.clone()),
        embed: (block_type == "embed").then_some(payload.clone()),
        equation: (block_type == "equation").then_some(payload.clone()),
        divider: (block_type == "divider").then_some(payload.clone()),
        code: (block_type == "code").then_some(payload.clone()),
        unsupported: (block_type == "unsupported").then_some(payload),
    }
}

pub(super) fn block_update(block_type: &str, payload: serde_json::Value) -> NoteBlockUpdate {
    NoteBlockUpdate {
        block_type: Some(block_type.to_string()),
        paragraph: (block_type == "paragraph").then_some(payload.clone()),
        heading_1: (block_type == "heading_1").then_some(payload.clone()),
        heading_2: (block_type == "heading_2").then_some(payload.clone()),
        heading_3: (block_type == "heading_3").then_some(payload.clone()),
        heading_4: (block_type == "heading_4").then_some(payload.clone()),
        heading_5: (block_type == "heading_5").then_some(payload.clone()),
        heading_6: (block_type == "heading_6").then_some(payload.clone()),
        bulleted_list_item: (block_type == "bulleted_list_item").then_some(payload.clone()),
        numbered_list_item: (block_type == "numbered_list_item").then_some(payload.clone()),
        to_do: (block_type == "to_do").then_some(payload.clone()),
        toggle: (block_type == "toggle").then_some(payload.clone()),
        callout: (block_type == "callout").then_some(payload.clone()),
        quote: (block_type == "quote").then_some(payload.clone()),
        child_page: (block_type == "child_page").then_some(payload.clone()),
        child_database: (block_type == "child_database").then_some(payload.clone()),
        breadcrumb: (block_type == "breadcrumb").then_some(payload.clone()),
        table_of_contents: (block_type == "table_of_contents").then_some(payload.clone()),
        column_list: (block_type == "column_list").then_some(payload.clone()),
        column: (block_type == "column").then_some(payload.clone()),
        table: (block_type == "table").then_some(payload.clone()),
        table_row: (block_type == "table_row").then_some(payload.clone()),
        tab: (block_type == "tab").then_some(payload.clone()),
        image: (block_type == "image").then_some(payload.clone()),
        video: (block_type == "video").then_some(payload.clone()),
        audio: (block_type == "audio").then_some(payload.clone()),
        file: (block_type == "file").then_some(payload.clone()),
        pdf: (block_type == "pdf").then_some(payload.clone()),
        bookmark: (block_type == "bookmark").then_some(payload.clone()),
        link_preview: (block_type == "link_preview").then_some(payload.clone()),
        synced_block: (block_type == "synced_block").then_some(payload.clone()),
        template: (block_type == "template").then_some(payload.clone()),
        button: (block_type == "button").then_some(payload.clone()),
        embed: (block_type == "embed").then_some(payload.clone()),
        equation: (block_type == "equation").then_some(payload.clone()),
        divider: (block_type == "divider").then_some(payload.clone()),
        code: (block_type == "code").then_some(payload.clone()),
        unsupported: (block_type == "unsupported").then_some(payload),
    }
}

pub(super) async fn create_page(pool: &SqlitePool, page_id: &str, block_id: &str) {
    writes::create_page(
        pool,
        NotePageCreate {
            id: page_id.to_string(),
            title: "First page".to_string(),
            parent: workspace_parent(),
            folder_id: None,
            first_block_id: block_id.to_string(),
            after_block_id: None,
            properties: None,
        },
    )
    .await
    .unwrap();
}

pub(super) async fn create_database(
    pool: &SqlitePool,
    database_id: &str,
    data_source_id: &str,
    view_id: &str,
    title: &str,
    after_block_id: &str,
) {
    databases::create_database(
        pool,
        NoteDatabaseCreate {
            id: database_id.to_string(),
            data_source_id: data_source_id.to_string(),
            view_id: view_id.to_string(),
            title: title.to_string(),
            parent: Some(page_parent(PAGE_A)),
            after_block_id: Some(after_block_id.to_string()),
            replace_block_id: None,
            icon: None,
            cover: None,
        },
    )
    .await
    .unwrap();
}
