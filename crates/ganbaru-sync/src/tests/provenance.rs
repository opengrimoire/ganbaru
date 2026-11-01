//! Row provenance: the devices that sealed a row, whatever its domain columns claim.

use super::block_on;
use super::replica::{Replica, device_id};
use crate::local;
use crate::manifest::vault::quick_notes::NOTES_TABLE;
use crate::provenance::row_creating_devices;

async fn add_note(replica: &mut Replica, id: &str) {
    replica
        .exec(&format!(
            "INSERT INTO quick_notes (id, title, order_key) VALUES ('{id}', 'claim', 'a0')"
        ))
        .await;
}

#[test]
fn row_creating_devices_returns_each_creator() {
    block_on(async {
        let mut left = Replica::new(0, 1).await;
        let mut right = Replica::new(1, 1).await;
        add_note(&mut left, "shared").await;
        add_note(&mut right, "shared").await;
        left.seal().await.unwrap();
        right.seal().await.unwrap();
        left.pull(&mut right).await;
        right.pull(&mut left).await;

        let expected = vec![device_id(0), device_id(1)];
        for replica in [&mut left, &mut right] {
            let devices = row_creating_devices(&mut replica.conn, NOTES_TABLE, "shared")
                .await
                .unwrap();
            assert_eq!(devices, expected);
        }

        local::reset_sync_state(&mut left.conn).await.unwrap();
        let devices = row_creating_devices(&mut left.conn, NOTES_TABLE, "shared")
            .await
            .unwrap();
        assert!(devices.is_empty());
    });
}
