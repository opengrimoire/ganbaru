//! A planned handover from the hub to a promoted desktop, and the proposals the hub holds for
//! an anchor it no longer is.

use super::{
    DirectTransport, HUB_DEVICE, PHONE_DEVICE, Replica, block_on, contact_source, device_key,
    hub_keys, open_hub, phone_ids, test_contact,
};
use crate::authority::{self, Enrollment};
use crate::client::{self, PeerIds};
use crate::hub::{HubForwards, SyncHub};
use crate::outbox::Outbox;
use crate::tests::{NOW_MS, VAULT_ID, acquire};
use ganbaru_sync::Engine;
use ganbaru_sync_contracts::{
    AdminKeyPair, DeviceKind, Digest32, ProposalRejection, SignedProposal, SpaceId,
};
use std::sync::Arc;
use tokio::sync::watch;

const LAPTOP_DEVICE: &str = "device-laptop";

fn laptop_ids() -> PeerIds {
    PeerIds {
        vault_id: VAULT_ID.to_string(),
        device_id: LAPTOP_DEVICE.to_string(),
    }
}

/// A hub whose service answers forward requests from the outbox of `replica`, as the app does.
fn forwarding_hub(replica: &Replica) -> DirectTransport {
    let (pool, ctx) = (replica.pool.clone(), replica.ctx.clone());
    let path = replica.directory.files().outbox_path();
    let forwards: HubForwards = Arc::new(move |requester, settled| {
        let (pool, ctx, path) = (pool.clone(), ctx.clone(), path.clone());
        Box::pin(async move {
            let mut outbox = Outbox::load(&path)?;
            let mut conn = acquire(&pool).await;
            authority::collect_forwards(
                &Engine::vault(),
                &mut conn,
                &ctx,
                &mut outbox,
                &requester,
                &settled,
            )
            .await
        })
    });
    DirectTransport(
        SyncHub::new(open_hub(replica), watch::Sender::new(0))
            .with_contact(contact_source(test_contact()))
            .with_forwards(forwards),
    )
}

/// Sequences a proposal through the writer of `replica`, which must anchor, and records the
/// commit.
async fn sequence_here(replica: &mut Replica, proposal: &SignedProposal) -> u64 {
    let mut conn = acquire(&replica.pool).await;
    let revision = authority::sequence(
        &replica.engine,
        &mut conn,
        &replica.ctx,
        &mut replica.writer.local(),
        proposal,
        NOW_MS,
    )
    .await
    .expect("sequence")
    .expect("the anchor accepts the proposal");
    drop(conn);
    commit_sequenced(replica).await;
    revision
}

/// Records the stored head of the replica's writer after sequencing through it.
async fn commit_sequenced(replica: &mut Replica) {
    let seq = replica.stored().await.get(&replica.writer.id());
    replica.writer.committed(seq).expect("record commit");
}

fn enroll_mobile(space: SpaceId, charter: Digest32, device_id: &str, seed: u8) -> SignedProposal {
    authority::propose(
        space,
        charter,
        &hub_keys().admin,
        authority::enroll_entry(Enrollment {
            device_id: device_id.to_string(),
            device_key: device_key(device_id).public_key(),
            tls_fingerprint: [seed; 32],
            label: device_id.to_string(),
            device_kind: DeviceKind::Mobile,
        }),
        NOW_MS + 1,
    )
    .expect("sign enrollment")
}

async fn hold(replica: &Replica, proposal: &SignedProposal) -> Result<u64, ProposalRejection> {
    let mut outbox = Outbox::load(&replica.directory.files().outbox_path()).expect("outbox");
    let mut conn = acquire(&replica.pool).await;
    authority::hold_for_anchor(
        &replica.engine,
        &mut conn,
        &replica.ctx,
        &mut outbox,
        proposal.clone(),
    )
    .await
    .expect("hold")
}

#[test]
fn a_promoted_desktop_takes_the_anchor_and_collects_what_the_hub_holds() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-handover", HUB_DEVICE).await;
        let mut laptop = Replica::new("laptop-handover", LAPTOP_DEVICE).await;
        let laptop_admin = AdminKeyPair::from_seed(&[29; 32]).expect("admin seed");
        let charter = hub_replica.charter().await;
        let space = hub_replica.ctx.space;

        // The anchor enrolls the laptop and promotes it with the admin key it sent at pairing.
        let enroll = authority::propose(
            space,
            charter,
            &hub_keys().admin,
            authority::enroll_entry(Enrollment {
                device_id: LAPTOP_DEVICE.to_string(),
                device_key: device_key(LAPTOP_DEVICE).public_key(),
                tls_fingerprint: [27; 32],
                label: "Laptop".to_string(),
                device_kind: DeviceKind::Desktop,
            }),
            NOW_MS,
        )
        .expect("sign enrollment");
        sequence_here(&mut hub_replica, &enroll).await;
        let promote = authority::propose(
            space,
            charter,
            &hub_keys().admin,
            authority::promote_entry(LAPTOP_DEVICE, laptop_admin.public_key()),
            NOW_MS,
        )
        .expect("sign promotion");
        sequence_here(&mut hub_replica, &promote).await;

        let transport = forwarding_hub(&hub_replica);
        laptop
            .exchange(&transport, &laptop_ids())
            .await
            .expect("pull the log");
        laptop.apply().await;
        let stored = laptop.stored().await;
        let mut conn = acquire(&laptop.pool).await;
        let view = laptop
            .engine
            .authorization(&mut conn, &laptop.ctx)
            .await
            .expect("read authorization")
            .expect("charter");
        assert_eq!(view.anchor_device_id, HUB_DEVICE);
        assert!(authority::is_admin(
            &view,
            LAPTOP_DEVICE,
            &laptop_admin.public_key()
        ));
        assert!(
            !authority::is_admin(&view, PHONE_DEVICE, &laptop_admin.public_key()),
            "a member holds no admin role"
        );
        let handover = authority::seal_handover_here(
            &laptop.engine,
            &mut conn,
            &laptop.ctx,
            &laptop_admin,
            &mut laptop.writer.local(),
            Some(stored),
            NOW_MS,
        )
        .await
        .expect("seal handover")
        .expect("the promoted desktop takes the anchor");
        drop(conn);
        commit_sequenced(&mut laptop).await;
        laptop
            .exchange(&transport, &laptop_ids())
            .await
            .expect("push the handover");
        hub_replica.apply().await;
        let hub_view = {
            let mut conn = acquire(&hub_replica.pool).await;
            hub_replica
                .engine
                .authorization(&mut conn, &hub_replica.ctx)
                .await
                .expect("read authorization")
                .expect("charter")
        };
        assert_eq!(
            (
                hub_view.anchor_device_id.as_str(),
                hub_view.epoch,
                hub_view.revision
            ),
            (LAPTOP_DEVICE, 1, handover)
        );

        // The hub no longer anchors: it holds what devices propose for the laptop to collect.
        let wearable = enroll_mobile(space, charter, "device-watch", 40);
        let duplicate = enroll_mobile(space, charter, PHONE_DEVICE, 41);
        for proposal in [&wearable, &duplicate] {
            assert_eq!(
                hold(&hub_replica, proposal).await,
                Err(ProposalRejection::NotAnchor)
            );
        }
        // Only the anchor collects them, and no one else can settle them on its behalf.
        assert!(
            client::forward(
                &transport,
                &phone_ids(),
                &[(wearable.hash(), ProposalRejection::UnknownDevice)]
            )
            .await
            .expect("forward")
            .is_empty()
        );
        let forwarded = client::forward(&transport, &laptop_ids(), &[])
            .await
            .expect("forward");
        assert_eq!(forwarded, vec![wearable.clone(), duplicate.clone()]);
        let mut conn = acquire(&laptop.pool).await;
        let rejected = authority::sequence_forwarded(
            &laptop.engine,
            &mut conn,
            &laptop.ctx,
            &mut laptop.writer.local(),
            &forwarded,
            NOW_MS,
        )
        .await
        .expect("sequence forwarded");
        drop(conn);
        commit_sequenced(&mut laptop).await;
        assert_eq!(
            rejected,
            vec![(duplicate.hash(), ProposalRejection::AlreadyEnrolled)]
        );

        laptop
            .exchange(&transport, &laptop_ids())
            .await
            .expect("push the entry");
        hub_replica.apply().await;
        assert!(
            client::forward(&transport, &laptop_ids(), &rejected)
                .await
                .expect("forward")
                .is_empty(),
            "the chain holds one proposal and the anchor rejected the other"
        );
        // The hub answers each sender from what it learned.
        assert!(
            hold(&hub_replica, &wearable)
                .await
                .is_ok_and(|revision| revision > handover)
        );
        assert_eq!(
            hold(&hub_replica, &duplicate).await,
            Err(ProposalRejection::AlreadyEnrolled)
        );
        hub_replica.close().await;
        laptop.close().await;
    });
}
