use super::{ChatLiveFollowRead, interpret};
use crate::vault::handoff::protocol::{
    CHAT_LIVE_NOT_HERE_CODE, CHAT_LIVE_UNAVAILABLE_CODE, ControlMessage,
    MAX_CHAT_LIVE_PAYLOAD_BYTES,
};
use ganbaru_chat_contracts::models::ChatLiveExecutionRead;

const ASSIGNMENT: &str = "assignment-1";

fn read(cursor: u64, settled: bool) -> ChatLiveExecutionRead {
    ChatLiveExecutionRead {
        assignment_id: ASSIGNMENT.to_string(),
        thread_id: None,
        turn_id: None,
        provider_family_id: None,
        model_id: None,
        items: Vec::new(),
        turns: Vec::new(),
        removed_item_ids: Vec::new(),
        change_cursor: cursor,
        has_more: false,
        reset_required: false,
        settled,
    }
}

fn frame(read: &ChatLiveExecutionRead) -> ControlMessage {
    ControlMessage::ChatLive {
        payload: serde_json::to_string(read).expect("encode"),
        change_cursor: read.change_cursor,
        has_more: read.has_more,
        reset_required: read.reset_required,
        settled: read.settled,
    }
}

fn error(code: &str) -> ControlMessage {
    ControlMessage::Error {
        code: code.to_string(),
        message: "refused".to_string(),
        retryable: false,
    }
}

#[test]
fn a_matching_page_is_shown() {
    let page = read(7, true);
    assert_eq!(
        interpret(Ok(Some(frame(&page))), ASSIGNMENT),
        ChatLiveFollowRead::Page {
            read: Box::new(page)
        }
    );
}

#[test]
fn a_page_of_other_work_or_a_frame_that_disagrees_is_refused() {
    let other = ChatLiveExecutionRead {
        assignment_id: "assignment-2".to_string(),
        ..read(7, false)
    };
    assert_eq!(
        interpret(Ok(Some(frame(&other))), ASSIGNMENT),
        ChatLiveFollowRead::Unreachable
    );
    let ControlMessage::ChatLive { payload, .. } = frame(&read(7, false)) else {
        unreachable!("a live frame");
    };
    let disagreeing = ControlMessage::ChatLive {
        payload,
        change_cursor: 8,
        has_more: false,
        reset_required: false,
        settled: true,
    };
    assert_eq!(
        interpret(Ok(Some(disagreeing)), ASSIGNMENT),
        ChatLiveFollowRead::Unreachable
    );
}

#[test]
fn a_malformed_or_oversized_page_is_refused() {
    let malformed = ControlMessage::ChatLive {
        payload: "{\"assignmentId\":".to_string(),
        change_cursor: 1,
        has_more: false,
        reset_required: false,
        settled: false,
    };
    assert_eq!(
        interpret(Ok(Some(malformed)), ASSIGNMENT),
        ChatLiveFollowRead::Unreachable
    );
    let oversized = ControlMessage::ChatLive {
        payload: "a".repeat(MAX_CHAT_LIVE_PAYLOAD_BYTES + 1),
        change_cursor: 1,
        has_more: false,
        reset_required: false,
        settled: false,
    };
    assert_eq!(
        interpret(Ok(Some(oversized)), ASSIGNMENT),
        ChatLiveFollowRead::Unreachable
    );
}

#[test]
fn work_the_device_does_not_run_is_not_served_and_other_failures_retry() {
    assert_eq!(
        interpret(Ok(Some(error(CHAT_LIVE_NOT_HERE_CODE))), ASSIGNMENT),
        ChatLiveFollowRead::NotServed
    );
    for response in [
        Ok(Some(error(CHAT_LIVE_UNAVAILABLE_CODE))),
        Ok(Some(error("device_quarantined"))),
        Ok(Some(ControlMessage::ActivationAcknowledged {
            transfer_id: "transfer-1".to_string(),
        })),
        Ok(None),
        Err("connect timed out".to_string()),
    ] {
        assert_eq!(
            interpret(response, ASSIGNMENT),
            ChatLiveFollowRead::Unreachable
        );
    }
}
