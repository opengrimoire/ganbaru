use ganbaru_chat::state::{
    SESSION_TRANSITIONS, TURN_TRANSITIONS, can_transition_session, can_transition_thread,
    can_transition_turn, thread_sources, transition_session, transition_thread, transition_turn,
    turn_sources,
};
use ganbaru_chat_contracts::models::{
    ChatErrorCode, ChatThreadState, ChatTurnState, ProviderSessionState,
};

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

#[test]
fn pending_turns_start_directly_when_the_provider_reports_activity() {
    assert!(can_transition_turn(
        ChatTurnState::Pending,
        ChatTurnState::Active
    ));
}

#[test]
fn pending_turns_settle_when_the_provider_completes_before_reporting_a_start() {
    assert!(can_transition_turn(
        ChatTurnState::Pending,
        ChatTurnState::Completed
    ));
}

#[test]
fn turn_sources_list_exactly_the_states_that_may_move_to_each_target() {
    for to in ChatTurnState::ALL {
        let sources: Vec<ChatTurnState> = turn_sources(to).collect();
        for from in ChatTurnState::ALL {
            assert_eq!(
                sources.contains(&from),
                TURN_TRANSITIONS.contains(&(from, to)),
                "turn source {from:?} for {to:?}"
            );
        }
    }
}

#[test]
fn running_turn_states_exclude_pending_and_terminal_states() {
    for state in ChatTurnState::ALL {
        assert_eq!(
            state.is_running(),
            state != ChatTurnState::Pending && !state.is_terminal(),
            "running {state:?}"
        );
    }
}

#[test]
fn thread_transitions_move_between_persisted_states_until_closed() {
    for from in ChatThreadState::ALL {
        for to in ChatThreadState::ALL {
            let expected = from != to
                && from.is_persisted()
                && to.is_persisted()
                && from != ChatThreadState::Closed;
            assert_eq!(
                can_transition_thread(from, to),
                expected,
                "thread transition {from:?} to {to:?}"
            );
            let result = transition_thread(from, to);
            assert_eq!(result.is_ok(), expected, "thread result {from:?} to {to:?}");
            if let Err(error) = result {
                assert_eq!(error.code, ChatErrorCode::InvalidStateTransition);
            }
        }
    }
}

#[test]
fn closed_threads_never_reopen_and_archived_is_never_a_stored_state() {
    assert!(ChatThreadState::Closed.is_terminal());
    assert!(!ChatThreadState::Archived.is_persisted());
    for state in ChatThreadState::ALL {
        assert!(!can_transition_thread(ChatThreadState::Closed, state));
        assert!(!can_transition_thread(state, ChatThreadState::Archived));
        assert!(!can_transition_thread(ChatThreadState::Archived, state));
    }
    for to in ChatThreadState::ALL {
        let sources: Vec<ChatThreadState> = thread_sources(to).collect();
        for from in ChatThreadState::ALL {
            assert_eq!(sources.contains(&from), can_transition_thread(from, to));
        }
    }
}

#[test]
fn state_sql_literals_match_their_wire_names() {
    for state in ChatTurnState::ALL {
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            serde_json::json!(state.as_str())
        );
    }
    for state in ChatThreadState::ALL {
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            serde_json::json!(state.as_str())
        );
    }
}
