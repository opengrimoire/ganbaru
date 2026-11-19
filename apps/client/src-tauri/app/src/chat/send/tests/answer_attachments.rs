//! Attachments on approval answers: resource lines, the wire shape, and thread readability.

use super::*;

fn answer_attachment(id: &str, display_name: &str) -> PromptAttachmentReference {
    PromptAttachmentReference {
        attachment_id: ChatAttachmentId::new(id).unwrap(),
        kind: "image".to_string(),
        display_name: display_name.to_string(),
        managed_relative_path: format!("assets/chat/attachments/{id}.png"),
        resource_uri: format!("ganbaru://chat/resource/{id}"),
        mime_type: Some("image/png".to_string()),
        byte_size: 14,
        local_path: None,
        text_content: None,
    }
}

#[test]
fn answer_attachments_append_resource_lines() {
    let answer = UserInputAnswer {
        question_id: "question-0".to_string(),
        selected_option_ids: vec!["option-1".to_string()],
        free_form_text: Some("See the screenshot".to_string()),
        attachment_ids: vec![
            ChatAttachmentId::new("attachment-a").unwrap(),
            ChatAttachmentId::new("attachment-b").unwrap(),
        ],
    };
    let attachments = [
        answer_attachment("attachment-a", "error.png"),
        answer_attachment("attachment-b", "two\nlines.png"),
    ];
    let resolved = answer_with_attachment_lines(answer, &attachments);
    assert_eq!(
        resolved.free_form_text.as_deref(),
        Some(
            "See the screenshot\nAttached: error.png ganbaru://chat/resource/attachment-a\nAttached: two lines.png ganbaru://chat/resource/attachment-b"
        )
    );
    assert!(resolved.attachment_ids.is_empty());
    assert_eq!(resolved.selected_option_ids, vec!["option-1".to_string()]);

    let without_text = UserInputAnswer {
        question_id: "question-1".to_string(),
        selected_option_ids: Vec::new(),
        free_form_text: None,
        attachment_ids: vec![ChatAttachmentId::new("attachment-a").unwrap()],
    };
    assert_eq!(
        answer_with_attachment_lines(without_text, &attachments[..1])
            .free_form_text
            .as_deref(),
        Some("Attached: error.png ganbaru://chat/resource/attachment-a")
    );

    let text_only = UserInputAnswer {
        question_id: "question-2".to_string(),
        selected_option_ids: Vec::new(),
        free_form_text: Some("Line\n".to_string()),
        attachment_ids: Vec::new(),
    };
    assert_eq!(
        answer_with_attachment_lines(text_only.clone(), &[]),
        text_only
    );
}

#[test]
fn answer_attachments_without_ids_keep_the_wire_shape() {
    let answer = UserInputAnswer {
        question_id: "question-0".to_string(),
        selected_option_ids: Vec::new(),
        free_form_text: None,
        attachment_ids: Vec::new(),
    };
    let wire = serde_json::to_value(&answer).unwrap();
    assert!(wire.get("attachmentIds").is_none());
    let parsed: UserInputAnswer = serde_json::from_value(serde_json::json!({
        "questionId": "question-0",
        "selectedOptionIds": [],
        "freeFormText": null,
    }))
    .unwrap();
    assert_eq!(parsed, answer);
}

#[test]
fn answer_attachments_become_readable_by_the_thread() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let thread_id = ChatThreadId::new("thread-1").unwrap();
        let attachment_id = ChatAttachmentId::new("attachment-answer").unwrap();
        sqlx::query(
            "INSERT INTO chat_attachments
                    (id, working_folder_id, kind, original_display_name, mime_type, byte_size,
                     sha256, managed_relative_path, signature_kind, created_at)
                 VALUES (?, 'workspace-1', 'image', 'answer.png', 'image/png', 14, ?, ?, 'png', ?)",
        )
        .bind(attachment_id.as_str())
        .bind("0".repeat(64))
        .bind("assets/chat/attachments/answer.png")
        .bind("2026-07-21T12:00:00Z")
        .execute(&pool)
        .await
        .unwrap();
        let now = UtcTimestamp::new("2026-07-21T12:01:00Z").unwrap();
        let mut connection = pool.acquire().await.unwrap();
        for _ in 0..2 {
            expose_attachments_to_thread(
                &mut connection,
                &thread_id,
                std::slice::from_ref(&attachment_id),
                &now,
            )
            .await
            .unwrap();
        }
        drop(connection);
        let references: Vec<(String, String)> =
            sqlx::query_as("SELECT resource_id, thread_id FROM chat_resource_thread_references")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            references,
            vec![(
                attachment_id.as_str().to_string(),
                thread_id.as_str().to_string()
            )]
        );
    });
}
