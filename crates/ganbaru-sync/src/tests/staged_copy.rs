//! Re-verification of a staged snapshot copy's authorization record store: every envelope is
//! admitted again under the charter the receiving device trusts, so a sender cannot plant rows,
//! hide records, or swap the charter, and the live store's verified envelopes merge back in.

use ganbaru_contacts::PersonKeyPair;
use ganbaru_sync_contracts::{Charter, SignedCharter, sign_charter};

use super::authorization::{admin_entry, view};
use super::block_on;
use super::replica::{
    ANCHOR_DEVICE_ID, Replica, START_MS, VAULT_ID, anchor_admin, anchor_device_key, device_id,
    person, recovery_admin,
};
use crate::{AuthorizationView, SyncError, local};

/// The installed charter of `replica`.
async fn charter_of(replica: &mut Replica) -> SignedCharter {
    replica
        .engine
        .charter(&mut replica.conn, &replica.ctx)
        .await
        .unwrap()
        .expect("test spaces have a charter")
}

/// Whether the selected chain of `view` removed the device of replica `index`.
fn removes(view: &AuthorizationView, index: usize) -> bool {
    view.devices
        .iter()
        .any(|device| device.device_id == device_id(index) && device.removed_revision.is_some())
}

/// A charter of the shared vault founded again after Reset sync state.
fn refounded_charter(replica: &Replica) -> SignedCharter {
    let person = person();
    sign_charter(
        &Charter {
            space: replica.ctx.space,
            anchor_device_id: ANCHOR_DEVICE_ID.to_owned(),
            anchor_device_key: anchor_device_key(&person).public_key(),
            anchor_tls_fingerprint: [3; 32],
            anchor_admin_key: anchor_admin(&person).public_key(),
            recovery_admin_key: recovery_admin(&person).public_key(),
            discovery_key: [4; 32],
            created_at_ms: (START_MS + 1) as i64,
        },
        &person,
    )
    .expect("test charter should sign")
}

#[test]
fn staged_copy_with_a_truncated_log_regains_the_removal() {
    block_on(async {
        let mut hub = Replica::anchor(0, 140).await;
        hub.remove(&device_id(3)).await.unwrap();
        let removal: Vec<u8> = sqlx::query_scalar(
            "SELECT envelope_hash FROM sync_auth_envelopes WHERE role = 'record'
             ORDER BY stored_order DESC LIMIT 1",
        )
        .fetch_one(&mut hub.conn)
        .await
        .unwrap();
        let mut live = hub.clone_installation(1).await;
        let mut staged = hub.clone_installation(2).await;

        // The sender keeps the removal's row but swaps its body for an earlier record's, so the
        // selected chain on the copy never removes device 3.
        sqlx::query(
            "UPDATE sync_auth_envelopes
             SET (envelope, entry_hash, previous_entry_hash) = (
                 SELECT envelope, entry_hash, previous_entry_hash FROM sync_auth_envelopes
                 WHERE role = 'record' ORDER BY stored_order LIMIT 1)
             WHERE role = 'record' AND envelope_hash = ?",
        )
        .bind(removal.as_slice())
        .execute(&mut staged.conn)
        .await
        .unwrap();
        crate::authorization::rederive(&mut staged.conn, staged.ctx.space)
            .await
            .unwrap();
        assert!(!removes(&view(&mut staged).await, 3));

        let charter = charter_of(&mut hub).await;
        let ctx = staged.ctx.clone();
        let report = staged
            .engine
            .reverify_staged_authorization(
                &mut staged.conn,
                Some(&mut live.conn),
                &ctx,
                &charter,
                START_MS,
            )
            .await
            .unwrap();
        assert_eq!((report.dropped, report.merged_from_live), (1, 1), "{report:?}");
        let stored: Vec<u8> = sqlx::query_scalar(
            "SELECT envelope FROM sync_auth_envelopes WHERE envelope_hash = ?",
        )
        .bind(removal.as_slice())
        .fetch_one(&mut staged.conn)
        .await
        .unwrap();
        let honest: Vec<u8> = sqlx::query_scalar(
            "SELECT envelope FROM sync_auth_envelopes WHERE envelope_hash = ?",
        )
        .bind(removal.as_slice())
        .fetch_one(&mut hub.conn)
        .await
        .unwrap();
        assert_eq!(stored, honest);
        assert!(removes(&view(&mut staged).await, 3));
        assert_eq!(view(&mut staged).await, view(&mut hub).await);
    });
}

#[test]
fn staged_copy_with_a_replaced_charter_is_refused() {
    block_on(async {
        let mut hub = Replica::anchor(0, 141).await;
        let other = PersonKeyPair::from_seed(&[9; 32]).unwrap();
        let mut staged = Replica::with_space(1, 141, VAULT_ID, other).await;
        let before = staged.snapshot().await;

        let charter = charter_of(&mut hub).await;
        let result = staged
            .engine
            .reverify_staged_authorization(
                &mut staged.conn,
                Some(&mut hub.conn),
                &hub.ctx,
                &charter,
                START_MS,
            )
            .await;
        assert!(
            matches!(result, Err(SyncError::SnapshotCharter)),
            "{result:?}"
        );
        assert_eq!(staged.snapshot().await, before);
    });
}

#[test]
fn forced_handover_copy_without_the_old_head_is_accepted() {
    block_on(async {
        let mut hub = Replica::anchor(0, 142).await;
        let mut member = Replica::new(3, 142).await;
        member.pull(&mut hub).await;
        hub.sequence(admin_entry(20)).await.unwrap().unwrap();
        let forced = member.handover_proposal(&recovery_admin(&person())).await;
        member.seal_handover(&forced, None).await.unwrap().unwrap();
        let mut live = Replica::new(4, 142).await;
        live.pull(&mut hub).await;
        let mut union = Replica::new(5, 142).await;
        union.pull(&mut hub).await;
        union.pull(&mut member).await;

        let charter = charter_of(&mut hub).await;
        let ctx = member.ctx.clone();
        let report = member
            .engine
            .reverify_staged_authorization(
                &mut member.conn,
                Some(&mut live.conn),
                &ctx,
                &charter,
                START_MS,
            )
            .await
            .unwrap();
        assert_eq!(report.dropped, 0, "{report:?}");
        let merged = view(&mut member).await;
        assert_eq!(merged, view(&mut union).await);
        assert_eq!(merged.anchor_device_id, device_id(3));
    });
}

#[test]
fn repair_after_reset_sync_state_accepts_the_new_charter() {
    block_on(async {
        let mut hub = Replica::anchor(0, 143).await;
        hub.remove(&device_id(3)).await.unwrap();
        let mut staged = hub.clone_installation(1).await;
        local::reset_sync_state(&mut staged.conn).await.unwrap();
        staged
            .engine
            .init_space(&mut staged.conn, VAULT_ID, START_MS)
            .await
            .unwrap();
        let charter = refounded_charter(&staged);
        let ctx = staged.ctx.clone();
        staged
            .engine
            .install_charter(&mut staged.conn, &ctx, &charter)
            .await
            .unwrap();

        // The old log's live store belongs to another charter, so nothing of it merges.
        let report = staged
            .engine
            .reverify_staged_authorization(
                &mut staged.conn,
                Some(&mut hub.conn),
                &ctx,
                &charter,
                START_MS,
            )
            .await
            .unwrap();
        assert_eq!(report.merged_from_live, 0, "{report:?}");
        assert_eq!(charter_of(&mut staged).await, charter);
        let refounded = view(&mut staged).await;
        assert_eq!(refounded.anchor_device_id, ANCHOR_DEVICE_ID);
        assert!(!removes(&refounded, 3));
    });
}
