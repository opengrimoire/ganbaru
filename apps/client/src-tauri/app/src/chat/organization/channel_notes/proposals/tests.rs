use super::super::{
    CreateChatChannelNoteCommand, DeleteChatChannelNoteCommand, MAX_ACTIVE_CHANNEL_NOTES,
    create_channel_note, delete_channel_note, list_channel_notes,
};
use super::*;
use ganbaru_chat::coordination::reports::{HostCall, ReportKind, record_host_report};
use ganbaru_chat::project_defaults::default_channel_ids;
use ganbaru_chat_contracts::models::ChatParticipantId;

const PROJECT: &str = "project-chat";
const AGENT: &str = "participant:folder-agent";
const RUN: &str = "run:working";
const ASSIGNMENT: &str = "assignment:folders";

/// A pool whose working agent run proposed `bodies` as channel notes, in order, with the
/// proposal item ids.
async fn pool_with_proposals(bodies: &[&str]) -> (SqlitePool, Vec<ChatConversationItemId>) {
    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    let mut items = Vec::with_capacity(bodies.len());
    for (index, body) in bodies.iter().enumerate() {
        let request = format!("proposal-{index}");
        let now = format!("2026-10-20T12:0{}:00.000Z", index + 1);
        let mut connection = pool.acquire().await.unwrap();
        record_host_report(
            &mut connection,
            HostCall {
                run_id: RUN,
                assignment_id: ASSIGNMENT,
                client_request_id: &request,
            },
            ReportKind::NoteProposal,
            body,
            &now,
        )
        .await
        .unwrap();
        let item: String = sqlx::query_scalar(
            "SELECT item_id FROM chat_work_semantic_updates
             WHERE update_kind = 'note_proposal' AND created_at = ?",
        )
        .bind(&now)
        .fetch_one(&mut *connection)
        .await
        .unwrap();
        items.push(ChatConversationItemId::new(item).unwrap());
    }
    (pool, items)
}

fn channel() -> ChatChannelId {
    ChatChannelId::new(default_channel_ids(PROJECT).unwrap().1).unwrap()
}

fn accept(
    command: &str,
    item: &ChatConversationItemId,
    body: Option<&str>,
) -> AcceptChatNoteProposalCommand {
    AcceptChatNoteProposalCommand {
        client_command_id: ChatCommandId::new(command).unwrap(),
        item_id: item.clone(),
        body: body.map(str::to_owned),
    }
}

fn dismiss(command: &str, item: &ChatConversationItemId) -> DismissChatNoteProposalCommand {
    DismissChatNoteProposalCommand {
        client_command_id: ChatCommandId::new(command).unwrap(),
        item_id: item.clone(),
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

async fn note_rows(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM chat_channel_notes")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn an_accepted_proposal_becomes_one_note_crediting_its_agent() {
    tauri::async_runtime::block_on(async {
        let (pool, items) = pool_with_proposals(&["Reply in English"]).await;
        let note = accept_note_proposal(&pool, &accept("accept-1", &items[0], None))
            .await
            .unwrap();
        assert_eq!(note.id, proposal_note_id(&items[0]).unwrap());
        assert_eq!(note.body, "Reply in English");
        assert_eq!(note.channel_id, channel());
        assert_eq!(note.source_item_id.as_ref(), Some(&items[0]));
        assert_eq!(
            note.proposed_by.as_ref().map(ChatParticipantId::as_str),
            Some(AGENT)
        );
        assert_eq!(
            note.edited_by.as_ref().map(ChatParticipantId::as_str),
            Some(LOCAL_PARTICIPANT_ID)
        );
        assert_eq!(note.created_at.as_str(), "2026-10-20T12:01:00.000Z");

        let replayed = accept_note_proposal(&pool, &accept("accept-1", &items[0], None))
            .await
            .unwrap();
        assert_eq!(replayed, note);
        let again = accept_note_proposal(&pool, &accept("accept-2", &items[0], Some("Other")))
            .await
            .unwrap();
        assert_eq!(again.id, note.id);
        assert_eq!(again.body, "Reply in English");
        assert_eq!(note_rows(&pool).await, 1);
        assert_eq!(listed_bodies(&pool).await, ["Reply in English"]);
    });
}

#[test]
fn a_proposal_is_accepted_with_the_persons_edited_text() {
    tauri::async_runtime::block_on(async {
        let (pool, items) = pool_with_proposals(&["Reply in English"]).await;
        let note = accept_note_proposal(
            &pool,
            &accept("accept-1", &items[0], Some("  Reply in Spanish \n")),
        )
        .await
        .unwrap();
        assert_eq!(note.body, "Reply in Spanish");
        assert_eq!(
            note.proposed_by.as_ref().map(ChatParticipantId::as_str),
            Some(AGENT)
        );
        let blank = accept_note_proposal(&pool, &accept("accept-2", &items[0], Some(" ")))
            .await
            .unwrap();
        assert_eq!(
            blank.body, "Reply in Spanish",
            "a saved note is returned as is"
        );
    });
}

#[test]
fn a_dismissed_proposal_is_never_saved() {
    tauri::async_runtime::block_on(async {
        let (pool, items) = pool_with_proposals(&["Reply in English", "Use pnpm"]).await;
        dismiss_note_proposal(&pool, &dismiss("dismiss-1", &items[0]))
            .await
            .unwrap();
        dismiss_note_proposal(&pool, &dismiss("dismiss-1", &items[0]))
            .await
            .unwrap();
        dismiss_note_proposal(&pool, &dismiss("dismiss-2", &items[0]))
            .await
            .unwrap();
        assert!(listed_bodies(&pool).await.is_empty());
        assert_eq!(
            note_rows(&pool).await,
            1,
            "the dismissal is one deleted row"
        );

        let refused = accept_note_proposal(&pool, &accept("accept-1", &items[0], None))
            .await
            .unwrap_err();
        assert_eq!(refused.code, ChatErrorCode::Conflict);

        accept_note_proposal(&pool, &accept("accept-2", &items[1], None))
            .await
            .unwrap();
        let saved = dismiss_note_proposal(&pool, &dismiss("dismiss-3", &items[1]))
            .await
            .unwrap_err();
        assert_eq!(saved.code, ChatErrorCode::Conflict);
        assert_eq!(listed_bodies(&pool).await, ["Use pnpm"]);
    });
}

#[test]
fn only_note_proposals_can_be_settled() {
    tauri::async_runtime::block_on(async {
        let (pool, _) = pool_with_proposals(&[]).await;
        for item in ["item:folders", "item:missing"] {
            let item = ChatConversationItemId::new(item).unwrap();
            for error in [
                accept_note_proposal(&pool, &accept("accept-1", &item, None))
                    .await
                    .unwrap_err(),
                dismiss_note_proposal(&pool, &dismiss("dismiss-1", &item))
                    .await
                    .unwrap_err(),
            ] {
                assert_eq!(error.code, ChatErrorCode::NotFound, "{error:?}");
                assert_eq!(error.field.as_deref(), Some("itemId"), "{error:?}");
            }
        }
    });
}

#[test]
fn accepting_a_proposal_respects_the_note_limit() {
    tauri::async_runtime::block_on(async {
        let (pool, items) = pool_with_proposals(&["Reply in English"]).await;
        for index in 0..MAX_ACTIVE_CHANNEL_NOTES {
            create_channel_note(
                &pool,
                &CreateChatChannelNoteCommand {
                    client_command_id: ChatCommandId::new(format!("create-{index}")).unwrap(),
                    channel_id: channel(),
                    body: format!("Fact {index}"),
                },
            )
            .await
            .unwrap();
        }
        let full = accept_note_proposal(&pool, &accept("accept-1", &items[0], None))
            .await
            .unwrap_err();
        assert_eq!(full.code, ChatErrorCode::Validation);
        assert_eq!(full.field.as_deref(), Some("channelId"));
        dismiss_note_proposal(&pool, &dismiss("dismiss-1", &items[0]))
            .await
            .unwrap();
    });
}

#[test]
fn only_members_settle_proposals() {
    tauri::async_runtime::block_on(async {
        let (pool, items) = pool_with_proposals(&["Reply in English"]).await;
        sqlx::query(
            "UPDATE chat_conversation_memberships SET removed_at = '2026-10-20T13:00:00.000Z'
             WHERE participant_id = ?",
        )
        .bind(LOCAL_PARTICIPANT_ID)
        .execute(&pool)
        .await
        .unwrap();
        for error in [
            accept_note_proposal(&pool, &accept("accept-1", &items[0], None))
                .await
                .unwrap_err(),
            dismiss_note_proposal(&pool, &dismiss("dismiss-1", &items[0]))
                .await
                .unwrap_err(),
        ] {
            assert_eq!(error.code, ChatErrorCode::Permission, "{error:?}");
        }
        assert_eq!(note_rows(&pool).await, 0);
    });
}

#[test]
fn proposal_states_follow_the_note_each_proposal_settles_into() {
    tauri::async_runtime::block_on(async {
        let (pool, items) =
            pool_with_proposals(&["Reply in English", "Use pnpm", "Ship on Tuesdays"]).await;
        let states = |pool: SqlitePool, items: Vec<ChatConversationItemId>| async move {
            read_note_proposal_states(&pool, items)
                .await
                .unwrap()
                .into_iter()
                .map(|read| read.state)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            states(pool.clone(), items.clone()).await,
            [ChatNoteProposalState::Pending; 3]
        );

        accept_note_proposal(&pool, &accept("accept-1", &items[0], None))
            .await
            .unwrap();
        let deleted = accept_note_proposal(&pool, &accept("accept-2", &items[1], None))
            .await
            .unwrap();
        delete_channel_note(
            &pool,
            &DeleteChatChannelNoteCommand {
                client_command_id: ChatCommandId::new("delete-1").unwrap(),
                note_id: deleted.id,
                expected_revision: deleted.revision,
            },
        )
        .await
        .unwrap();
        dismiss_note_proposal(&pool, &dismiss("dismiss-1", &items[2]))
            .await
            .unwrap();
        let reads = read_note_proposal_states(&pool, items.clone())
            .await
            .unwrap();
        assert_eq!(
            reads
                .iter()
                .map(|read| read.proposal_item_id.clone())
                .collect::<Vec<_>>(),
            items,
            "states keep the order they were asked in"
        );
        assert_eq!(
            states(pool.clone(), items).await,
            [
                ChatNoteProposalState::Saved,
                ChatNoteProposalState::Dismissed,
                ChatNoteProposalState::Dismissed,
            ]
        );
    });
}

#[test]
fn proposal_state_reads_are_bounded() {
    tauri::async_runtime::block_on(async {
        let (pool, items) = pool_with_proposals(&["Reply in English"]).await;
        let at_limit = vec![items[0].clone(); MAX_NOTE_PROPOSAL_STATE_READS];
        assert_eq!(
            read_note_proposal_states(&pool, at_limit)
                .await
                .unwrap()
                .len(),
            MAX_NOTE_PROPOSAL_STATE_READS
        );
        let over = vec![items[0].clone(); MAX_NOTE_PROPOSAL_STATE_READS + 1];
        let error = read_note_proposal_states(&pool, over).await.unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation);
        assert_eq!(error.field.as_deref(), Some("proposalItemIds"));
        assert!(
            read_note_proposal_states(&pool, Vec::new())
                .await
                .unwrap()
                .is_empty()
        );
    });
}
