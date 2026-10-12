#[test]
fn mutation_result_preserves_camel_case_wire_shape() {
    let result = super::super::NotesMutationResultDto {
        value: "saved",
        next_history_checkpoint_at: Some("2026-07-12T12:00:00.000Z".to_string()),
    };

    assert_eq!(
        serde_json::to_value(result).unwrap(),
        serde_json::json!({
            "value": "saved",
            "nextHistoryCheckpointAt": "2026-07-12T12:00:00.000Z"
        })
    );
}
