//! Installation helpers: space context, writer high-water check, snapshot copies, and
//! carry-forward across a database replacement.

use super::replica::{Replica, VAULT_ID, connection_from, person};
use super::{block_on, migrated_pool};
use crate::local::{
    self, LocalWriterHead, decode_bundle, encode_bundle, has_pending_captures,
    prepare_snapshot_copy, replacement_vector, space_context,
};
use crate::log::WriterState;

async fn add_note(replica: &mut Replica, id: &str, title: &str) {
    replica
        .exec(&format!(
            "INSERT INTO quick_notes (id, title, order_key) VALUES ('{id}', '{title}', 'a0')"
        ))
        .await;
}

#[test]
fn space_context_needs_the_person_identity() {
    block_on(async {
        let mut replica = Replica::new(0, 1).await;
        assert!(
            space_context(&mut replica.conn, VAULT_ID)
                .await
                .unwrap()
                .is_none()
        );
        replica
            .exec(&format!(
                "INSERT INTO contacts_local_identity
                    (singleton, public_key, card_nonce, created_at, updated_at)
                 VALUES (1, '{}', 'AAAAAAAAAAAAAAAAAAAAAA', '2026-01-01T00:00:00Z',
                    '2026-01-01T00:00:00Z')",
                person().public_key().to_text()
            ))
            .await;
        let ctx = space_context(&mut replica.conn, VAULT_ID)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ctx, replica.ctx);
    });
}

#[test]
fn writer_head_tracks_the_sealed_chain_and_detects_tampering() {
    block_on(async {
        let mut replica = Replica::new(0, 2).await;
        let key = replica.identity.key.public_key();
        let space = replica.ctx.space;
        let head = replica
            .engine
            .local_writer_head(&mut replica.conn, space, &key)
            .await
            .unwrap();
        assert_eq!(head, LocalWriterHead::Absent);

        add_note(&mut replica, "note-1", "One").await;
        replica.seal().await.unwrap();
        add_note(&mut replica, "note-2", "Two").await;
        let last = replica.seal().await.unwrap().last_seq.unwrap();
        assert!(last >= 2);
        let head = replica
            .engine
            .local_writer_head(&mut replica.conn, space, &key)
            .await
            .unwrap();
        assert_eq!(
            head,
            LocalWriterHead::Genuine {
                stored: last,
                state: WriterState::Active
            }
        );

        replica
            .exec(&format!(
                "UPDATE sync_ops SET envelope = (SELECT envelope FROM sync_ops AS earlier
                    WHERE earlier.writer_id = sync_ops.writer_id AND earlier.seq = {})
                 WHERE seq = {last}",
                last - 1
            ))
            .await;
        let head = replica
            .engine
            .local_writer_head(&mut replica.conn, space, &key)
            .await
            .unwrap();
        assert_eq!(head, LocalWriterHead::Mismatch);
    });
}

#[test]
fn snapshot_copies_drop_unsealed_captures() {
    block_on(async {
        let mut replica = Replica::new(0, 3).await;
        add_note(&mut replica, "note-1", "One").await;
        assert!(has_pending_captures(&mut replica.conn).await.unwrap());
        let mut copy = connection_from(&replica.snapshot().await).await;
        prepare_snapshot_copy(&mut copy).await.unwrap();
        assert!(!has_pending_captures(&mut copy).await.unwrap());
        assert!(has_pending_captures(&mut replica.conn).await.unwrap());
    });
}

#[test]
fn carry_forward_moves_local_operations_into_a_replacement() {
    block_on(async {
        let mut hub = Replica::new(0, 4).await;
        let mut device = Replica::new(1, 4).await;
        add_note(&mut hub, "note-hub", "Hub").await;
        hub.seal().await.unwrap();
        device.pull(&mut hub).await;
        add_note(&mut device, "note-device", "Device").await;
        let device_last = device.seal().await.unwrap().last_seq.unwrap();
        add_note(&mut hub, "note-later", "Later").await;
        hub.seal().await.unwrap();

        // The replacement is the hub's newer snapshot, which lacks the device's operations.
        let mut replacement = connection_from(&hub.snapshot().await).await;
        let staged = replacement_vector(&mut replacement, device.ctx.space)
            .await
            .unwrap();
        let ops = device
            .engine
            .carry_forward_ops(&mut device.conn, &device.ctx, &staged)
            .await
            .unwrap();
        assert_eq!(u64::try_from(ops.len()).unwrap(), device_last);

        let (space, decoded) = decode_bundle(&encode_bundle(device.ctx.space, &ops)).unwrap();
        assert_eq!(space, device.ctx.space);
        assert_eq!(decoded, ops);

        for _ in 0..2 {
            let report = device
                .engine
                .import_carried(
                    &mut replacement,
                    &device.ctx,
                    VAULT_ID,
                    &decoded,
                    device.now_ms,
                )
                .await
                .unwrap();
            assert_eq!(report.refused, 0);
            assert!(!report.pending_captures);
        }
        let titles: Vec<String> = sqlx::query_scalar("SELECT title FROM quick_notes ORDER BY id")
            .fetch_all(&mut replacement)
            .await
            .unwrap();
        assert_eq!(titles, ["Device", "Hub", "Later"]);
        let head = device
            .engine
            .local_writer_head(
                &mut replacement,
                device.ctx.space,
                &device.identity.key.public_key(),
            )
            .await
            .unwrap();
        assert_eq!(
            head,
            LocalWriterHead::Genuine {
                stored: device_last,
                state: WriterState::Active
            }
        );

        // The device keeps sealing on the replacement.
        device.conn = replacement;
        add_note(&mut device, "note-after", "After").await;
        assert_eq!(device.seal().await.unwrap().last_seq, Some(device_last + 1));
        device.check_invariants().await;
    });
}

#[test]
fn carry_forward_from_a_database_without_a_space_is_empty() {
    block_on(async {
        let pool = migrated_pool().await;
        let mut fresh = pool.acquire().await.unwrap();
        let replica = Replica::new(1, 5).await;
        let ops = replica
            .engine
            .carry_forward_ops(&mut fresh, &replica.ctx, &Default::default())
            .await
            .unwrap();
        assert!(ops.is_empty());
        assert!(
            replacement_vector(&mut fresh, replica.ctx.space)
                .await
                .unwrap()
                .iter()
                .next()
                .is_none()
        );
    });
}

#[test]
fn bundles_refuse_truncated_or_foreign_input() {
    let space = ganbaru_sync_contracts::SpaceId::personal(VAULT_ID);
    let bytes = encode_bundle(space, &[vec![1, 2, 3], vec![4]]);
    assert_eq!(
        decode_bundle(&bytes).unwrap(),
        (space, vec![vec![1, 2, 3], vec![4]])
    );
    assert!(decode_bundle(&bytes[..bytes.len() - 1]).is_err());
    assert!(decode_bundle(b"NOTABUNDLE").is_err());
    assert!(
        decode_bundle(&encode_bundle(space, &[]))
            .unwrap()
            .1
            .is_empty()
    );
    let _ = local::MAX_BUNDLE_BYTES;
}
