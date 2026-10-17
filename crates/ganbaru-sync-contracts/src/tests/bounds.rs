use super::fixtures::{group_value, header, row, space, writer_id, writer_key};
use crate::bounds::{
    MAX_BLOB_FIELD_BYTES, MAX_CHANGES_PER_OPERATION, MAX_DEPENDENCIES, MAX_FIELDS_PER_VALUE,
    MAX_HEADER_BYTES, MAX_OPERATION_BYTES, MAX_ROW_KEY_BYTES, MAX_TEXT_FIELD_BYTES,
};
use crate::codec::CodecError;
use crate::{
    Change, ChangeAction, Content, Envelope, Field, Operation, OperationError, TableId, Value,
    WriterId,
};

const PREVIOUS_HASH: [u8; 32] = [7; 32];

fn changes_operation(changes: Vec<Change>, dependencies: Vec<(WriterId, u64)>) -> Operation {
    Operation {
        space: space(),
        writer: writer_id(),
        seq: 2,
        previous_hash: PREVIOUS_HASH,
        header: header(1_790_000_000_000, dependencies),
        content: Content::Changes(changes),
    }
}

fn tombstone(index: usize) -> Change {
    Change {
        table: TableId(1),
        row: row(&format!("row-{index}")),
        action: ChangeAction::Tombstone,
        groups: Vec::new(),
        replaced_by: None,
    }
}

fn write(key: &str, fields: Vec<Field>) -> Change {
    Change {
        table: TableId(2),
        row: row(key),
        action: ChangeAction::Write,
        groups: vec![group_value(2, 0, fields)],
        replaced_by: None,
    }
}

fn dependency_writer(index: usize) -> WriterId {
    let mut bytes = [0u8; 16];
    bytes[14..].copy_from_slice(&(index as u16 + 1).to_be_bytes());
    WriterId::from_bytes(bytes)
}

fn codec_error(result: Result<impl Sized, OperationError>) -> CodecError {
    match result {
        Err(OperationError::Codec(error)) => error,
        Err(error) => panic!("unexpected error {error}"),
        Ok(_) => panic!("operation sealed past a bound"),
    }
}

#[test]
fn values_hold_one_to_sixteen_fields() {
    assert_eq!(
        Value::new(Vec::new()),
        Err(CodecError::Bounds("value fields"))
    );
    assert!(Value::new(vec![Field::Null; MAX_FIELDS_PER_VALUE]).is_ok());
    assert_eq!(
        Value::new(vec![Field::Null; MAX_FIELDS_PER_VALUE + 1]),
        Err(CodecError::Bounds("value fields"))
    );
    let mut oversized = vec![(MAX_FIELDS_PER_VALUE + 1) as u8];
    oversized.extend(std::iter::repeat_n(0u8, MAX_FIELDS_PER_VALUE + 1));
    assert_eq!(
        Value::decode(&oversized),
        Err(CodecError::Bounds("value fields"))
    );
    assert_eq!(Value::decode(&[0]), Err(CodecError::Bounds("value fields")));
}

#[test]
fn text_and_blob_fields_stop_at_their_limits() {
    let text = Field::Text("x".repeat(MAX_TEXT_FIELD_BYTES));
    let value = Value::new(vec![text]).unwrap();
    assert_eq!(Value::decode(&value.encode()).unwrap(), value);
    assert_eq!(
        Value::new(vec![Field::Text("x".repeat(MAX_TEXT_FIELD_BYTES + 1))]),
        Err(CodecError::Bounds("text field"))
    );
    let blob = Value::new(vec![Field::Blob(vec![1; MAX_BLOB_FIELD_BYTES])]).unwrap();
    assert_eq!(Value::decode(&blob.encode()).unwrap(), blob);
    assert_eq!(
        Value::new(vec![Field::Blob(vec![1; MAX_BLOB_FIELD_BYTES + 1])]),
        Err(CodecError::Bounds("blob field"))
    );

    let mut oversized_text = vec![1, 2];
    oversized_text.extend_from_slice(&((MAX_TEXT_FIELD_BYTES + 1) as u32).to_be_bytes());
    oversized_text.extend(std::iter::repeat_n(b'x', MAX_TEXT_FIELD_BYTES + 1));
    assert_eq!(
        Value::decode(&oversized_text),
        Err(CodecError::Bounds("text field"))
    );
    assert_eq!(
        Value::decode(&[1, 2, 0, 0, 0, 2, 0xc3, 0x28]),
        Err(CodecError::Malformed("text field"))
    );
    assert_eq!(
        Value::decode(&[1, 9]),
        Err(CodecError::Malformed("field tag"))
    );
}

#[test]
fn operations_hold_one_to_the_maximum_changes() {
    let key = writer_key();
    assert_eq!(
        codec_error(changes_operation(Vec::new(), Vec::new()).seal(&key)),
        CodecError::Bounds("changes")
    );
    let full: Vec<Change> = (0..MAX_CHANGES_PER_OPERATION).map(tombstone).collect();
    let sealed = changes_operation(full, Vec::new()).seal(&key).unwrap();
    let decoded = Envelope::decode(sealed.bytes())
        .unwrap()
        .operation()
        .unwrap();
    match decoded.content {
        Content::Changes(changes) => assert_eq!(changes.len(), MAX_CHANGES_PER_OPERATION),
        other => panic!("unexpected content {other:?}"),
    }
    let over: Vec<Change> = (0..=MAX_CHANGES_PER_OPERATION).map(tombstone).collect();
    assert_eq!(
        codec_error(changes_operation(over, Vec::new()).seal(&key)),
        CodecError::Bounds("changes")
    );
}

#[test]
fn a_row_may_change_once_per_operation() {
    let changes = vec![tombstone(1), tombstone(1)];
    assert_eq!(
        codec_error(changes_operation(changes, Vec::new()).seal(&writer_key())),
        CodecError::Malformed("duplicate row change")
    );
}

#[test]
fn dependencies_stop_at_their_limit() {
    let key = writer_key();
    let full: Vec<(WriterId, u64)> = (0..MAX_DEPENDENCIES)
        .map(|index| (dependency_writer(index), 1))
        .collect();
    assert!(
        changes_operation(vec![tombstone(0)], full)
            .seal(&key)
            .is_ok()
    );
    let over: Vec<(WriterId, u64)> = (0..=MAX_DEPENDENCIES)
        .map(|index| (dependency_writer(index), 1))
        .collect();
    assert_eq!(
        codec_error(changes_operation(vec![tombstone(0)], over).seal(&key)),
        CodecError::Bounds("dependencies")
    );
    let own = vec![(writer_id(), 1)];
    assert_eq!(
        codec_error(changes_operation(vec![tombstone(0)], own).seal(&key)),
        CodecError::Malformed("dependencies")
    );
}

#[test]
fn headers_stop_at_their_limit_before_the_operation_does() {
    let long_key = "k".repeat(MAX_ROW_KEY_BYTES - 5);
    let changes: Vec<Change> = (0..MAX_CHANGES_PER_OPERATION)
        .map(|index| write(&format!("{long_key}{index:05}"), vec![Field::Integer(1)]))
        .collect();
    assert_eq!(
        codec_error(changes_operation(changes, Vec::new()).seal(&writer_key())),
        CodecError::Bounds("header")
    );
    const { assert!(MAX_HEADER_BYTES < MAX_OPERATION_BYTES) };
}

#[test]
fn operations_stop_at_their_size_limit() {
    let key = writer_key();
    let half = "x".repeat(MAX_TEXT_FIELD_BYTES);
    let oversized = vec![write(
        "note-1",
        vec![Field::Text(half.clone()), Field::Text(half)],
    )];
    assert_eq!(
        codec_error(changes_operation(oversized, Vec::new()).seal(&key)),
        CodecError::Bounds("operation")
    );

    let largest_note = vec![write(
        "note-1",
        vec![
            Field::Text("A title".into()),
            Field::Blob(vec![0x5a; 290_000]),
        ],
    )];
    let sealed = changes_operation(largest_note, Vec::new())
        .seal(&key)
        .unwrap();
    assert!(sealed.bytes().len() <= MAX_OPERATION_BYTES);
    let envelope = Envelope::decode(sealed.bytes()).unwrap();
    assert!(envelope.verify(&key.public_key()));
    assert!(envelope.operation().is_ok());
}

#[test]
fn decoding_rejects_oversized_envelopes() {
    let bytes = vec![0u8; MAX_OPERATION_BYTES + 1];
    assert_eq!(
        Envelope::decode(&bytes).err(),
        Some(CodecError::Bounds("operation"))
    );
}
