//! Managed assets through the hub: transfer ahead of the rows that reference them, the pass
//! byte bound, upload verification, class checks, damaged assets, and collection grace.

use super::{
    DirectTransport, HUB_DEVICE, PHONE_DEVICE, Replica, TABLET_DEVICE, contact_source, done,
    error_code, hub, open_hub_serving, phone_ids, png, received, reference, tablet_ids,
    test_contact,
};
use crate::assets::{
    ASSET_UNAVAILABLE, AssetMemory, AssetStore, AssetSweep, AssetTransfer, ContentAddressedStore,
    INVALID_ASSET, InFlightAssets, MAX_ASSET_PASS_BYTES, UNREFERENCED_ASSET_GRACE,
    collect_unreferenced_assets,
};
use crate::backup::ArchiveGate;
use crate::client::SyncTransport;
use crate::hub::SyncHub;
use crate::tests::{VAULT_ID, acquire, add_note, block_on};
use crate::wire::{asset_key, asset_to_wire, bytes_to_wire};
use ganbaru_handoff::protocol::{
    ControlMessage, PROTOCOL_VERSION, SYNC_ASSET_CHUNK_BYTES, SyncAsset,
};
use ganbaru_quick_notes::images;
use ganbaru_sync::Engine;
use ganbaru_sync::assets::AssetRef;
use ganbaru_sync::manifest::vault::quick_notes::IMAGES_CLASS;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::watch;

fn put(image: &AssetRef, offset: u32, bytes: &[u8]) -> ControlMessage {
    ControlMessage::SyncAssetPut {
        protocol_version: PROTOCOL_VERSION,
        vault_id: VAULT_ID.to_string(),
        device_id: PHONE_DEVICE.to_string(),
        asset: asset_to_wire(image),
        offset,
        data: bytes_to_wire(bytes),
    }
}

#[test]
fn assets_travel_through_the_hub_ahead_of_the_rows_that_reference_them() {
    block_on(async {
        let hub_replica = Replica::new("images-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("images-phone", PHONE_DEVICE).await;
        let mut tablet = Replica::new("images-tablet", TABLET_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        add_note(&mut *acquire(&phone.pool).await, "note-images").await;
        let large = png(SYNC_ASSET_CHUNK_BYTES * 2 + 17, 1);
        let small = png(64, 2);
        let large_image = phone.add_image("note-images", 0, &large).await;
        let small_image = phone.add_image("note-images", 1, &small).await;
        phone.seal().await;

        let mut phone_memory = AssetMemory::default();
        let uploaded = phone
            .upload(
                &transport,
                &phone_ids(),
                &mut phone_memory,
                MAX_ASSET_PASS_BYTES,
            )
            .await;
        assert_eq!(
            (uploaded.uploaded, uploaded.more, uploaded.problem),
            (2, false, None)
        );
        assert_eq!(hub_replica.asset_bytes(&large_image), Some(large.clone()));
        done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        let again = phone
            .upload(
                &transport,
                &phone_ids(),
                &mut phone_memory,
                MAX_ASSET_PASS_BYTES,
            )
            .await;
        assert_eq!(again, AssetTransfer::default());

        done(
            tablet
                .exchange(&transport, &tablet_ids())
                .await
                .expect("exchange"),
        );
        tablet.apply().await;
        assert_eq!(tablet.asset_bytes(&large_image), None);
        let mut tablet_memory = AssetMemory::default();
        let downloaded = tablet
            .download(&transport, &tablet_ids(), &mut tablet_memory)
            .await;
        assert_eq!((downloaded.downloaded, downloaded.problem), (2, None));
        assert_eq!(tablet.asset_bytes(&large_image), Some(large));
        assert_eq!(tablet.asset_bytes(&small_image), Some(small));
        let again = tablet
            .download(&transport, &tablet_ids(), &mut tablet_memory)
            .await;
        assert_eq!(again, AssetTransfer::default());

        hub_replica.close().await;
        phone.close().await;
        tablet.close().await;
    });
}

#[test]
fn assets_stop_transferring_at_the_pass_byte_bound_and_resume() {
    block_on(async {
        let hub_replica = Replica::new("bound-hub", HUB_DEVICE).await;
        let phone = Replica::new("bound-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        add_note(&mut *acquire(&phone.pool).await, "note-images").await;
        phone.add_image("note-images", 0, &png(100, 1)).await;
        phone.add_image("note-images", 1, &png(100, 2)).await;

        let mut memory = AssetMemory::default();
        let first = phone.upload(&transport, &phone_ids(), &mut memory, 1).await;
        assert_eq!((first.uploaded, first.more), (1, true));
        let second = phone.upload(&transport, &phone_ids(), &mut memory, 1).await;
        assert_eq!((second.uploaded, second.more), (1, false));
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn the_hub_verifies_asset_uploads_and_reports_missing_assets() {
    block_on(async {
        let hub_replica = Replica::new("verify-hub", HUB_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        let bytes = png(SYNC_ASSET_CHUNK_BYTES + 10, 3);
        let image = reference(&bytes);

        let get = ControlMessage::SyncAssetGet {
            protocol_version: PROTOCOL_VERSION,
            vault_id: VAULT_ID.to_string(),
            device_id: PHONE_DEVICE.to_string(),
            asset: asset_to_wire(&image),
            offset: 0,
        };
        let missing = transport.request(get.clone()).await.expect("get");
        assert_eq!(error_code(missing), ASSET_UNAVAILABLE);

        let mut tampered = bytes.clone();
        *tampered.last_mut().expect("bytes") ^= 0xff;
        let (head, tail) = tampered.split_at(SYNC_ASSET_CHUNK_BYTES);
        let progress = transport.request(put(&image, 0, head)).await.expect("put");
        assert_eq!(received(progress), SYNC_ASSET_CHUNK_BYTES as u32);
        let refused = transport
            .request(put(&image, SYNC_ASSET_CHUNK_BYTES as u32, tail))
            .await
            .expect("put");
        assert_eq!(error_code(refused), INVALID_ASSET);
        assert_eq!(hub_replica.asset_bytes(&image), None);
        assert_eq!(hub_replica.class_entries(), Vec::<String>::new());

        // A chunk that does not continue the pending upload learns where to restart.
        let (head, tail) = bytes.split_at(SYNC_ASSET_CHUNK_BYTES);
        let restart = transport
            .request(put(&image, SYNC_ASSET_CHUNK_BYTES as u32, tail))
            .await
            .expect("put");
        assert_eq!(received(restart), 0);
        transport.request(put(&image, 0, head)).await.expect("put");
        let stored = transport
            .request(put(&image, SYNC_ASSET_CHUNK_BYTES as u32, tail))
            .await
            .expect("put");
        assert_eq!(received(stored), image.byte_size());
        assert_eq!(hub_replica.asset_bytes(&image), Some(bytes.clone()));
        let held = transport.request(put(&image, 0, head)).await.expect("put");
        assert_eq!(received(held), image.byte_size());
        match transport.request(get).await.expect("get") {
            ControlMessage::SyncAssetChunk { data } => {
                assert_eq!(data, bytes_to_wire(head));
            }
            other => panic!("expected a chunk, got {other:?}"),
        }
        hub_replica.close().await;
    });
}

#[test]
fn the_hub_refuses_assets_outside_their_registered_class_before_staging() {
    block_on(async {
        let hub_replica = Replica::new("class-hub", HUB_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        let bytes = png(64, 5);
        let image = reference(&bytes);
        let put_wire = |asset: SyncAsset| ControlMessage::SyncAssetPut {
            protocol_version: PROTOCOL_VERSION,
            vault_id: VAULT_ID.to_string(),
            device_id: PHONE_DEVICE.to_string(),
            asset,
            offset: 0,
            data: bytes_to_wire(&bytes),
        };

        let unregistered = SyncAsset {
            class: "unregistered/files".to_string(),
            ..asset_to_wire(&image)
        };
        let refused = transport
            .request(put_wire(unregistered))
            .await
            .expect("put");
        assert_eq!(error_code(refused), "invalid_sync_request");

        let oversized = SyncAsset {
            byte_size: IMAGES_CLASS.max_bytes + 1,
            ..asset_to_wire(&image)
        };
        let refused = transport.request(put_wire(oversized)).await.expect("put");
        assert_eq!(error_code(refused), "invalid_sync_request");
        assert_eq!(hub_replica.class_entries(), Vec::<String>::new());
        assert_eq!(hub_replica.asset_bytes(&image), None);
        hub_replica.close().await;
    });
}

#[test]
fn a_replica_refuses_damaged_assets_from_the_hub() {
    block_on(async {
        let mut hub_replica = Replica::new("damaged-hub", HUB_DEVICE).await;
        let mut tablet = Replica::new("damaged-tablet", TABLET_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        add_note(&mut *acquire(&hub_replica.pool).await, "note-images").await;
        let image = hub_replica.add_image("note-images", 0, &png(80, 4)).await;
        hub_replica.seal().await;
        let path = image.path(hub_replica.directory.path());
        let mut damaged = std::fs::read(&path).expect("read image");
        *damaged.last_mut().expect("bytes") ^= 0xff;
        std::fs::write(&path, damaged).expect("damage image");

        done(
            tablet
                .exchange(&transport, &tablet_ids())
                .await
                .expect("exchange"),
        );
        tablet.apply().await;
        let mut memory = AssetMemory::default();
        let refused = tablet
            .download(&transport, &tablet_ids(), &mut memory)
            .await;
        assert_eq!(refused.downloaded, 0);
        assert!(
            refused
                .problem
                .as_deref()
                .is_some_and(|problem| problem.contains("do not match")),
            "{refused:?}"
        );
        assert_eq!(tablet.asset_bytes(&image), None);
        assert_eq!(tablet.class_entries(), Vec::<String>::new());
        let skipped = tablet
            .download(&transport, &tablet_ids(), &mut memory)
            .await;
        assert_eq!(skipped, AssetTransfer::default());
        hub_replica.close().await;
        tablet.close().await;
    });
}

#[test]
fn asset_query_does_not_renew_collection_grace() {
    block_on(async {
        let hub_replica = Replica::new("grace-hub", HUB_DEVICE).await;
        let bytes = png(64, 6);
        images::import_image(hub_replica.directory.path(), &bytes).expect("store image");
        let image = reference(&bytes);
        let in_flight = Arc::new(InFlightAssets::default());
        let store: Arc<dyn AssetStore> = Arc::new(
            ContentAddressedStore::new(hub_replica.directory.path().to_path_buf())
                .with_in_flight(Arc::clone(&in_flight)),
        );
        let transport = DirectTransport(
            SyncHub::new(
                open_hub_serving(&hub_replica, BTreeSet::new(), store),
                watch::Sender::new(0),
            )
            .with_contact(contact_source(test_contact())),
        );

        // A peer keeps asking about an unreferenced file to keep it on disk.
        for _ in 0..3 {
            let query = ControlMessage::SyncAssetQuery {
                protocol_version: PROTOCOL_VERSION,
                vault_id: VAULT_ID.to_string(),
                device_id: PHONE_DEVICE.to_string(),
                assets: vec![asset_to_wire(&image)],
            };
            match transport.request(query).await.expect("query") {
                ControlMessage::SyncAssetPresence { held } => {
                    assert_eq!(held, vec![asset_key(&image)]);
                }
                other => panic!("expected the presence answer, got {other:?}"),
            }
        }

        let engine = Engine::vault();
        let swept = collect_unreferenced_assets(
            &ArchiveGate::default(),
            AssetSweep {
                engine: &engine,
                pool: &hub_replica.pool,
                vault_root: hub_replica.directory.path(),
                in_flight: &in_flight,
                grace: UNREFERENCED_ASSET_GRACE,
                now: SystemTime::now() + UNREFERENCED_ASSET_GRACE * 2,
            },
            |_| Some(()),
        )
        .await;
        assert_eq!(swept, Ok(1), "a question keeps no file alive");
        assert_eq!(hub_replica.asset_bytes(&image), None);
        hub_replica.close().await;
    });
}
