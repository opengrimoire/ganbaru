use super::super::tests::{migrated_pool, seed_test_channel};
use super::*;
use ganbaru_chat::conversion::membership_key;
use ganbaru_chat_contracts::models::UtcTimestamp;

const CHANNEL: &str = "channel:test";
const CONVERSATION: &str = "conversation:test";
const SEEDED_AT: &str = "2026-11-12T10:00:00.000Z";

/// A pool holding the test channel with the local person as a member.
async fn member_pool() -> SqlitePool {
    let pool = migrated_pool().await;
    seed_test_channel(&pool, &UtcTimestamp::new(SEEDED_AT).unwrap()).await;
    sqlx::query(
        "INSERT INTO chat_conversation_memberships
            (conversation_id, participant_id, sync_key, membership_role, created_at, updated_at)
         VALUES (?, ?, ?, 'owner', ?, ?)",
    )
    .bind(CONVERSATION)
    .bind(LOCAL_PARTICIPANT_ID)
    .bind(membership_key(CONVERSATION, LOCAL_PARTICIPANT_ID).unwrap())
    .bind(SEEDED_AT)
    .bind(SEEDED_AT)
    .execute(&pool)
    .await
    .unwrap();
    pool
}

fn channel() -> ChatChannelId {
    ChatChannelId::new(CHANNEL).unwrap()
}

fn command_id(value: &str) -> ChatCommandId {
    ChatCommandId::new(value).unwrap()
}

fn create(command: &str, body: &str) -> CreateChatChannelNoteCommand {
    CreateChatChannelNoteCommand {
        client_command_id: command_id(command),
        channel_id: channel(),
        body: body.to_owned(),
    }
}

fn update(
    command: &str,
    note: &ChatChannelNoteRead,
    expected_revision: u64,
    body: &str,
) -> UpdateChatChannelNoteCommand {
    UpdateChatChannelNoteCommand {
        client_command_id: command_id(command),
        note_id: note.id.clone(),
        expected_revision,
        body: body.to_owned(),
    }
}

fn delete(
    command: &str,
    note: &ChatChannelNoteRead,
    expected_revision: u64,
) -> DeleteChatChannelNoteCommand {
    DeleteChatChannelNoteCommand {
        client_command_id: command_id(command),
        note_id: note.id.clone(),
        expected_revision,
    }
}

async fn listed_bodies(pool: &SqlitePool) -> Vec<String> {
    list_channel_notes(pool, &channel())
        .await
        .unwrap()
        .into_iter()
        .map(|note| note.body)
        .collect()
}

#[test]
fn channel_notes_save_reword_and_delete_as_the_local_person() {
    tauri::async_runtime::block_on(async {
        let pool = member_pool().await;
        let note = create_channel_note(&pool, &create("create-1", "  Reply in English \n"))
            .await
            .unwrap();
        assert_eq!(note.body, "Reply in English");
        assert_eq!(note.revision, 1);
        assert_eq!(note.channel_id, channel());
        assert_eq!(
            note.edited_by.as_ref().map(ChatParticipantId::as_str),
            Some(LOCAL_PARTICIPANT_ID)
        );
        assert_eq!(note.source_item_id, None);
        assert_eq!(note.proposed_by, None);

        let reworded =
            update_channel_note(&pool, &update("update-1", &note, 1, "Reply in Spanish"))
                .await
                .unwrap();
        assert_eq!(reworded.body, "Reply in Spanish");
        assert_eq!(reworded.revision, 2);
        assert_eq!(listed_bodies(&pool).await, ["Reply in Spanish"]);

        delete_channel_note(&pool, &delete("delete-1", &reworded, 2))
            .await
            .unwrap();
        assert!(listed_bodies(&pool).await.is_empty());
        let deleted: Option<String> =
            sqlx::query_scalar("SELECT deleted_at FROM chat_channel_notes WHERE id = ?")
                .bind(note.id.as_str())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(deleted.is_some(), "the deletion stays as a row");
    });
}

#[test]
fn channel_notes_list_newest_updated_first() {
    tauri::async_runtime::block_on(async {
        let pool = member_pool().await;
        let first = create_channel_note(&pool, &create("create-1", "First"))
            .await
            .unwrap();
        create_channel_note(&pool, &create("create-2", "Second"))
            .await
            .unwrap();
        sqlx::query("UPDATE chat_channel_notes SET updated_at = ? WHERE id <> ?")
            .bind(SEEDED_AT)
            .bind(first.id.as_str())
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(listed_bodies(&pool).await, ["First", "Second"]);
        update_channel_note(&pool, &update("update-1", &first, 1, "First again"))
            .await
            .unwrap();
        sqlx::query("UPDATE chat_channel_notes SET updated_at = ? WHERE id = ?")
            .bind(SEEDED_AT)
            .bind(first.id.as_str())
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE chat_channel_notes SET updated_at = ? WHERE id <> ?")
            .bind("2026-11-12T11:00:00.000Z")
            .bind(first.id.as_str())
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(listed_bodies(&pool).await, ["Second", "First again"]);
    });
}

#[test]
fn channel_note_edits_at_a_stale_revision_are_refused() {
    tauri::async_runtime::block_on(async {
        let pool = member_pool().await;
        let note = create_channel_note(&pool, &create("create-1", "Reply in English"))
            .await
            .unwrap();
        update_channel_note(&pool, &update("update-1", &note, 1, "Reply in Spanish"))
            .await
            .unwrap();
        let stale = update_channel_note(&pool, &update("update-2", &note, 1, "Reply in French"))
            .await
            .unwrap_err();
        assert_eq!(stale.code, ChatErrorCode::StaleRevision);
        let stale_delete = delete_channel_note(&pool, &delete("delete-1", &note, 1))
            .await
            .unwrap_err();
        assert_eq!(stale_delete.code, ChatErrorCode::StaleRevision);
        assert_eq!(listed_bodies(&pool).await, ["Reply in Spanish"]);

        delete_channel_note(&pool, &delete("delete-2", &note, 2))
            .await
            .unwrap();
        let missing = update_channel_note(&pool, &update("update-3", &note, 3, "Back"))
            .await
            .unwrap_err();
        assert_eq!(missing.code, ChatErrorCode::NotFound);
    });
}

#[test]
fn channel_note_bodies_are_trimmed_bounded_and_plain() {
    tauri::async_runtime::block_on(async {
        let pool = member_pool().await;
        let longest = "n".repeat(MAX_CHANNEL_NOTE_CHARS);
        let note = create_channel_note(&pool, &create("create-1", &longest))
            .await
            .unwrap();
        assert_eq!(note.body.chars().count(), MAX_CHANNEL_NOTE_CHARS);
        let multibyte = "é".repeat(MAX_CHANNEL_NOTE_CHARS);
        create_channel_note(&pool, &create("create-2", &multibyte))
            .await
            .unwrap();
        for (index, body) in [
            " \n\t ".to_owned(),
            "n".repeat(MAX_CHANNEL_NOTE_CHARS + 1),
            "bell\u{7}".to_owned(),
        ]
        .iter()
        .enumerate()
        {
            let error = create_channel_note(&pool, &create(&format!("refused-{index}"), body))
                .await
                .unwrap_err();
            assert_eq!(error.code, ChatErrorCode::Validation, "{body:?}");
            assert_eq!(error.field.as_deref(), Some("body"), "{body:?}");
        }
        assert_eq!(
            normalized_note_body("Line one\n\tLine two").unwrap(),
            "Line one\n\tLine two"
        );
    });
}

#[test]
fn repeated_channel_note_commands_return_their_first_result() {
    tauri::async_runtime::block_on(async {
        let pool = member_pool().await;
        let note = create_channel_note(&pool, &create("create-1", "Reply in English"))
            .await
            .unwrap();
        let again = create_channel_note(&pool, &create("create-1", "Reply in English"))
            .await
            .unwrap();
        assert_eq!(again.id, note.id);
        assert_eq!(listed_bodies(&pool).await, ["Reply in English"]);

        update_channel_note(&pool, &update("update-1", &note, 1, "Reply in Spanish"))
            .await
            .unwrap();
        let replayed =
            update_channel_note(&pool, &update("update-1", &note, 1, "Reply in Spanish"))
                .await
                .unwrap();
        assert_eq!(replayed.revision, 2);

        let reused = delete_channel_note(&pool, &delete("create-1", &note, 2))
            .await
            .unwrap_err();
        assert_eq!(reused.code, ChatErrorCode::Conflict);
        delete_channel_note(&pool, &delete("delete-1", &note, 2))
            .await
            .unwrap();
        delete_channel_note(&pool, &delete("delete-1", &note, 2))
            .await
            .unwrap();
        let receipts: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_organizational_command_receipts
             WHERE command_kind = 'channel_note'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(receipts, 3);
    });
}

#[test]
fn a_channel_keeps_at_most_fifty_live_notes() {
    tauri::async_runtime::block_on(async {
        let pool = member_pool().await;
        let mut first = None;
        for index in 0..MAX_ACTIVE_CHANNEL_NOTES {
            let note = create_channel_note(
                &pool,
                &create(&format!("create-{index}"), &format!("Fact {index}")),
            )
            .await
            .unwrap();
            first.get_or_insert(note);
        }
        let full = create_channel_note(&pool, &create("create-full", "One more"))
            .await
            .unwrap_err();
        assert_eq!(full.code, ChatErrorCode::Validation);
        assert_eq!(full.field.as_deref(), Some("channelId"));

        let first = first.unwrap();
        delete_channel_note(&pool, &delete("delete-first", &first, 1))
            .await
            .unwrap();
        create_channel_note(&pool, &create("create-full", "One more"))
            .await
            .unwrap();
    });
}

#[test]
fn channel_notes_belong_to_members_of_active_channels() {
    tauri::async_runtime::block_on(async {
        let pool = member_pool().await;
        let note = create_channel_note(&pool, &create("create-1", "Reply in English"))
            .await
            .unwrap();

        sqlx::query("UPDATE chat_channels SET archived_at = ? WHERE id = ?")
            .bind(SEEDED_AT)
            .bind(CHANNEL)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(listed_bodies(&pool).await, ["Reply in English"]);
        let archived = create_channel_note(&pool, &create("create-2", "Another"))
            .await
            .unwrap_err();
        assert_eq!(archived.code, ChatErrorCode::Conflict);
        sqlx::query("UPDATE chat_channels SET archived_at = NULL WHERE id = ?")
            .bind(CHANNEL)
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "UPDATE chat_conversation_memberships SET removed_at = ?
             WHERE conversation_id = ? AND participant_id = ?",
        )
        .bind(SEEDED_AT)
        .bind(CONVERSATION)
        .bind(LOCAL_PARTICIPANT_ID)
        .execute(&pool)
        .await
        .unwrap();
        for error in [
            list_channel_notes(&pool, &channel()).await.unwrap_err(),
            create_channel_note(&pool, &create("create-3", "Another"))
                .await
                .unwrap_err(),
            update_channel_note(&pool, &update("update-1", &note, 1, "Reply in Spanish"))
                .await
                .unwrap_err(),
            delete_channel_note(&pool, &delete("delete-1", &note, 1))
                .await
                .unwrap_err(),
        ] {
            assert_eq!(error.code, ChatErrorCode::Permission, "{error:?}");
        }

        let unknown = list_channel_notes(&pool, &ChatChannelId::new("channel:missing").unwrap())
            .await
            .unwrap_err();
        assert_eq!(unknown.code, ChatErrorCode::NotFound);
    });
}
