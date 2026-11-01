//! Row provenance: the devices that sealed a row, whatever its domain columns claim.

use super::block_on;
use super::replica::{Replica, device_id};
use crate::local;
use crate::manifest::vault::quick_notes::NOTES_TABLE;
use crate::provenance::{device_labels, row_creating_devices};

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

#[test]
fn a_row_names_the_sealing_device_whatever_its_columns_claim() {
    block_on(async {
        let mut sender = Replica::new(0, 1).await;
        let mut reader = Replica::new(1, 1).await;
        add_note(&mut sender, "note").await;
        sender.seal().await.unwrap();
        reader.pull(&mut sender).await;
        assert_eq!(
            row_creating_devices(&mut reader.conn, NOTES_TABLE, "note")
                .await
                .unwrap(),
            vec![device_id(0)]
        );

        reader
            .exec("UPDATE quick_notes SET title = 'edited' WHERE id = 'note'")
            .await;
        reader.seal().await.unwrap();
        reader.apply_all().await.unwrap();
        assert_eq!(
            row_creating_devices(&mut reader.conn, NOTES_TABLE, "note")
                .await
                .unwrap(),
            vec![device_id(0)]
        );
        assert!(
            row_creating_devices(&mut reader.conn, NOTES_TABLE, "absent")
                .await
                .unwrap()
                .is_empty()
        );
    });
}

#[test]
fn device_labels_name_enrolled_devices() {
    block_on(async {
        let mut replica = Replica::new(0, 1).await;
        let labels = device_labels(&mut replica.conn).await.unwrap();
        assert_eq!(
            labels.get(&device_id(1)).map(String::as_str),
            Some("Device 1")
        );
    });
}
