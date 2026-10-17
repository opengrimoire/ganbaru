use ganbaru_chat::state::{
    SESSION_TRANSITIONS, TURN_TRANSITIONS, can_transition_session, can_transition_turn,
    transition_session, transition_turn,
};
use ganbaru_chat_contracts::models::{ChatErrorCode, ChatTurnState, ProviderSessionState};

#[test]
fn session_transition_table_accepts_only_declared_pairs() {
    for from in ProviderSessionState::ALL {
        for to in ProviderSessionState::ALL {
            let expected = SESSION_TRANSITIONS.contains(&(from, to));
            assert_eq!(
                can_transition_session(from, to),
                expected,
                "session transition {from:?} to {to:?}"
            );
            let result = transition_session(from, to);
            assert_eq!(
                result.is_ok(),
                expected,
                "session transition result {from:?} to {to:?}"
            );
            if let Err(error) = result {
                assert_eq!(error.code, ChatErrorCode::InvalidStateTransition);
            }
        }
    }
}

#[test]
fn turn_transition_table_accepts_only_declared_pairs() {
    for from in ChatTurnState::ALL {
        for to in ChatTurnState::ALL {
            let expected = TURN_TRANSITIONS.contains(&(from, to));
            assert_eq!(
                can_transition_turn(from, to),
                expected,
                "turn transition {from:?} to {to:?}"
            );
            let result = transition_turn(from, to);
            assert_eq!(
                result.is_ok(),
                expected,
                "turn transition result {from:?} to {to:?}"
            );
            if let Err(error) = result {
                assert_eq!(error.code, ChatErrorCode::InvalidStateTransition);
            }
        }
    }
}

#[test]
fn terminal_turn_states_have_no_outgoing_transitions() {
    for terminal in [
        ChatTurnState::Completed,
        ChatTurnState::Interrupted,
        ChatTurnState::Failed,
    ] {
        assert!(terminal.is_terminal());
        for next in ChatTurnState::ALL {
            assert!(!can_transition_turn(terminal, next));
        }
    }
}

#[test]
fn state_wire_literals_are_stable() {
    assert_eq!(
        serde_json::to_value(ProviderSessionState::WaitingForApproval).unwrap(),
        serde_json::json!("waiting_for_approval")
    );
    assert_eq!(
        serde_json::to_value(ChatTurnState::WaitingForUserInput).unwrap(),
        serde_json::json!("waiting_for_user_input")
    );
}
