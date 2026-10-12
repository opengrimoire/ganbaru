#[test]
fn guard_message_matches_the_vault_read_only_error() {
    assert_eq!(
        ganbaru_sync::guards::READ_ONLY_MESSAGE,
        crate::vault::ownership::READ_ONLY_ERROR
    );
}
