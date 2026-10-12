use super::{
    NOW_MS, TestDirectory, VAULT_ID, acquire, add_note, block_on, context, create_vault, person,
};
use crate::client::{
    ClientSession, Exchange, Exchanged, MAX_EXCHANGE_BYTES, PeerIds, SyncTransport, wait,
};
use crate::hub::{HubVault, OpenHubVault, SyncHub};
use crate::recovery::{RecoveryAction, RecoveryChoice};
use crate::writer::{ActiveWriter, SuccessorPlan, create_writer};
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_handoff::protocol::ControlMessage;
use ganbaru_sync::manifest::vault::quick_notes::NOTES_TABLE;
use ganbaru_sync::{Engine, SpaceContext};
use ganbaru_sync_contracts::op::RevokeReason;
use ganbaru_sync_contracts::{VersionVector, WriterId};
use sqlx::SqlitePool;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

const HUB_DEVICE: &str = "device-hub";
const PHONE_DEVICE: &str = "device-phone";

/// Calls the hub directly, validating both frames as the transport would.
struct DirectTransport(SyncHub);

impl SyncTransport for DirectTransport {
    async fn request(&self, message: ControlMessage) -> Result<ControlMessage, String> {
        message.validate()?;
        let response = self.0.respond(message).await;
        response.validate()?;
        Ok(response)
    }
}

/// One replica: its vault database and the writer of its installation.
struct Replica {
    directory: TestDirectory,
    registry: DatabasePoolRegistry,
    pool: SqlitePool,
    ctx: SpaceContext,
    writer: ActiveWriter,
}

impl Replica {
    async fn new(label: &str, device_id: &str) -> Self {
        let directory = TestDirectory::new(label);
        let registry = DatabasePoolRegistry::default();
        let pool = create_vault(&registry, &directory.path().join("vault.sqlite")).await;
        Self::open(directory, registry, pool, device_id).await
    }

    async fn open(
        directory: TestDirectory,
        registry: DatabasePoolRegistry,
        pool: SqlitePool,
        device_id: &str,
    ) -> Self {
        let ctx = context(&mut *acquire(&pool).await).await;
        let writer = create_writer(
            &directory.files(),
            &directory.keys(),
            SuccessorPlan::default(),
            &person(),
            device_id,
            NOW_MS,
        )
        .expect("create writer");
        Self {
            directory,
            registry,
            pool,
            ctx,
            writer,
        }
    }

    async fn seal_note(&mut self, id: &str) -> u64 {
        add_note(&mut *acquire(&self.pool).await, id).await;
        self.seal().await
    }

    /// Runs a local edit and seals it.
    async fn edit(&mut self, sql: &str, id: &str) {
        sqlx::query(sql)
            .bind(id)
            .execute(&self.pool)
            .await
            .expect("edit");
        self.seal().await;
    }

    async fn seal(&mut self) -> u64 {
        let mut conn = acquire(&self.pool).await;
        let seq = Engine::vault()
            .seal(&mut conn, &self.ctx, &mut self.writer.local(), NOW_MS)
            .await
            .expect("seal")
            .last_seq
            .expect("sealed operations");
        self.writer.committed(seq).expect("record commit");
        seq
    }

    async fn apply(&mut self) {
        let mut conn = acquire(&self.pool).await;
        let report = Engine::vault()
            .apply(&mut conn, &self.ctx, Some(&mut self.writer.local()), NOW_MS)
            .await
            .expect("apply");
        assert!(!report.more, "test logs apply in one round");
    }

    async fn stored(&self) -> VersionVector {
        Engine::vault()
            .stored_vector(&mut *acquire(&self.pool).await, &self.ctx)
            .await
            .expect("stored vector")
    }

    async fn has_note(&self, id: &str) -> bool {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM quick_notes WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .expect("count notes")
            > 0
    }

    async fn exchange(
        &self,
        transport: &DirectTransport,
        ids: &PeerIds,
    ) -> Result<Exchange, String> {
        let own_writers = self.writer.own_writers();
        self.exchange_with(
            transport,
            ids,
            self.writer.id(),
            &own_writers,
            MAX_EXCHANGE_BYTES,
        )
        .await
    }

    async fn exchange_with(
        &self,
        transport: &DirectTransport,
        ids: &PeerIds,
        active: WriterId,
        own_writers: &BTreeSet<WriterId>,
        max_bytes: usize,
    ) -> Result<Exchange, String> {
        let engine = Engine::vault();
        ClientSession {
            transport,
            ids,
            engine: &engine,
            pool: &self.pool,
            ctx: &self.ctx,
            writer: Some(active),
            own_writers,
            max_bytes,
        }
        .exchange()
        .await
    }

    async fn close(self) {
        drop(self.pool);
        self.registry.close_all().await.expect("close pools");
    }
}

fn phone_ids() -> PeerIds {
    PeerIds {
        vault_id: VAULT_ID.to_string(),
        device_id: PHONE_DEVICE.to_string(),
    }
}

/// A hub serving `pool`, with the change signal its service would bump.
fn hub(pool: &SqlitePool) -> (DirectTransport, watch::Sender<u64>) {
    let pool = pool.clone();
    let open: OpenHubVault = Arc::new(move || {
        let pool = pool.clone();
        Box::pin(async move {
            Ok(Some(HubVault {
                pool,
                vault_id: VAULT_ID.to_string(),
            }))
        })
    });
    let changes = watch::Sender::new(0);
    (
        DirectTransport(SyncHub::new(open, changes.clone())),
        changes,
    )
}

fn done(exchange: Exchange) -> Exchanged {
    match exchange {
        Exchange::Done(done) => done,
        Exchange::Fork(fork) => panic!("unexpected fork: {fork:?}"),
    }
}

fn copy_database(from: &Path, to: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let source = Path::new(&format!("{}{suffix}", from.display())).to_path_buf();
        if source.exists() {
            std::fs::copy(&source, format!("{}{suffix}", to.display())).expect("copy database");
        }
    }
}

#[test]
fn exchange_pushes_and_pulls_until_both_replicas_hold_every_operation() {
    block_on(async {
        let mut hub_replica = Replica::new("hub", HUB_DEVICE).await;
        let mut phone = Replica::new("phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica.pool);
        hub_replica.seal_note("note-hub").await;
        phone.seal_note("note-phone").await;

        let exchanged = done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        assert!(exchanged.pushed > 0);
        assert!(exchanged.pulled > 0);
        assert_eq!(exchanged.problem, None);
        assert_eq!(exchanged.known, hub_replica.stored().await);
        assert_eq!(phone.stored().await, hub_replica.stored().await);

        hub_replica.apply().await;
        phone.apply().await;
        for replica in [&hub_replica, &phone] {
            assert!(replica.has_note("note-hub").await);
            assert!(replica.has_note("note-phone").await);
        }

        let again = done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        assert_eq!((again.pushed, again.pulled), (0, 0));
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn an_exchange_stops_at_its_byte_bound_and_the_next_one_continues() {
    block_on(async {
        let mut hub_replica = Replica::new("hub", HUB_DEVICE).await;
        let mut phone = Replica::new("phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica.pool);
        for index in 0..3 {
            hub_replica.seal_note(&format!("note-{index}")).await;
        }
        let own_writers = phone.writer.own_writers();

        let first = done(
            phone
                .exchange_with(&transport, &phone_ids(), phone.writer.id(), &own_writers, 1)
                .await
                .expect("bounded exchange"),
        );
        assert!(first.more);
        assert_eq!(first.problem, None);
        assert_eq!(first.pulled, 1);
        assert_ne!(phone.stored().await, hub_replica.stored().await);

        let mut exchanges = 1;
        loop {
            let next = done(
                phone
                    .exchange_with(&transport, &phone_ids(), phone.writer.id(), &own_writers, 1)
                    .await
                    .expect("bounded exchange"),
            );
            exchanges += 1;
            assert_eq!(next.problem, None);
            if !next.more {
                break;
            }
            assert!(exchanges < 64, "bounded exchanges did not finish");
        }
        assert_eq!(phone.stored().await, hub_replica.stored().await);
        phone.apply().await;
        for index in 0..3 {
            assert!(phone.has_note(&format!("note-{index}")).await);
        }

        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn wait_returns_once_the_hub_stores_new_operations() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-wait", HUB_DEVICE).await;
        let phone = Replica::new("phone-wait", PHONE_DEVICE).await;
        let (transport, changes) = hub(&hub_replica.pool);
        hub_replica.seal_note("note-first").await;
        let known = done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        )
        .known;

        let ids = phone_ids();
        let (answer, ()) = tokio::join!(wait(&transport, &ids, &known), async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            hub_replica.seal_note("note-second").await;
            changes.send_modify(|generation| *generation += 1);
        });
        let answer = answer.expect("wait");
        assert!(!known.dominates(&answer));
        assert_eq!(answer, hub_replica.stored().await);
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn hub_refuses_foreign_geneses_unlinked_writers_and_other_vaults() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-refuse", HUB_DEVICE).await;
        let mut phone = Replica::new("phone-refuse", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica.pool);
        phone.seal_note("note-phone").await;

        let other_vault = PeerIds {
            vault_id: "vault-other".to_string(),
            device_id: PHONE_DEVICE.to_string(),
        };
        let error = phone
            .exchange(&transport, &other_vault)
            .await
            .expect_err("another vault is refused");
        assert!(error.contains("vault_mismatch"), "{error}");

        let impostor = PeerIds {
            vault_id: VAULT_ID.to_string(),
            device_id: "device-other".to_string(),
        };
        let refused = done(
            phone
                .exchange(&transport, &impostor)
                .await
                .expect("exchange"),
        );
        assert!(
            refused
                .problem
                .as_deref()
                .is_some_and(|problem| problem.contains("UnknownWriter")),
            "{refused:?}"
        );
        assert_eq!(hub_replica.stored().await.get(&phone.writer.id()), 0);

        done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        // A revocation keeps every applied operation, so the hub applies the genesis first.
        hub_replica.apply().await;
        let cutoff = hub_replica.stored().await.get(&phone.writer.id());
        assert!(cutoff > 0);
        {
            let mut conn = acquire(&hub_replica.pool).await;
            let seq = Engine::vault()
                .seal_revoke(
                    &mut conn,
                    &hub_replica.ctx,
                    phone.writer.id(),
                    cutoff,
                    RevokeReason::Revoked,
                    &mut hub_replica.writer.local(),
                    NOW_MS,
                )
                .await
                .expect("revoke")
                .last_seq
                .expect("revocation sealed");
            hub_replica.writer.committed(seq).expect("record commit");
        }
        phone.seal_note("note-after-unlink").await;
        let refused = done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        assert!(
            refused
                .problem
                .as_deref()
                .is_some_and(|problem| problem.contains("Revoked")),
            "{refused:?}"
        );
        assert_eq!(hub_replica.stored().await.get(&phone.writer.id()), cutoff);
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn a_cloned_installation_is_found_and_resealed_by_a_successor() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-fork", HUB_DEVICE).await;
        let mut original = Replica::new("phone-fork", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica.pool);
        let shared = original.seal_note("note-shared").await;
        done(
            original
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );

        // A disk clone: the same writer continues in two copies of the database.
        let original_path = original.directory.path().join("vault.sqlite");
        drop(original.pool);
        original.registry.close_all().await.expect("close original");
        let clone_directory = TestDirectory::new("phone-clone");
        let clone_path = clone_directory.path().join("vault.sqlite");
        copy_database(&original_path, &clone_path);
        original.pool = original
            .registry
            .connect_path(&original_path)
            .await
            .expect("reopen original");
        let clone_registry = DatabasePoolRegistry::default();
        let clone_pool = clone_registry
            .connect_path(&clone_path)
            .await
            .expect("open clone");

        let forked_seq = original.seal_note("note-original").await;
        let mut conn = acquire(&clone_pool).await;
        add_note(&mut conn, "note-clone").await;
        let clone_seq = Engine::vault()
            .seal(
                &mut conn,
                &original.ctx,
                &mut original.writer.local(),
                NOW_MS,
            )
            .await
            .expect("seal clone")
            .last_seq
            .expect("clone operations");
        drop(conn);
        assert_eq!(clone_seq, forked_seq);
        done(
            original
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );

        let forked_writer = original.writer.id();
        let mut clone =
            Replica::open(clone_directory, clone_registry, clone_pool, PHONE_DEVICE).await;
        // The clone still seals with the shared writer, so its probe reveals the fork.
        let clone_writers = BTreeSet::from([forked_writer]);
        let fork = match clone
            .exchange_with(
                &transport,
                &phone_ids(),
                forked_writer,
                &clone_writers,
                MAX_EXCHANGE_BYTES,
            )
            .await
            .expect("exchange")
        {
            Exchange::Fork(fork) => fork,
            Exchange::Done(done) => panic!("the fork went unnoticed: {done:?}"),
        };
        assert_eq!(fork.writer, forked_writer);
        assert_eq!(fork.keep_through, shared);

        {
            let mut conn = acquire(&clone.pool).await;
            let report = Engine::vault()
                .reseal_fork(
                    &mut conn,
                    &clone.ctx,
                    fork.writer,
                    fork.keep_through,
                    &mut clone.writer.local(),
                    NOW_MS,
                )
                .await
                .expect("re-seal fork");
            assert!(report.replaced_ops > 0);
            let seq = report.seal.last_seq.expect("re-sealed operations");
            clone.writer.committed(seq).expect("record commit");
        }
        let clone_writers = BTreeSet::from([forked_writer, clone.writer.id()]);
        let exchanged = done(
            clone
                .exchange_with(
                    &transport,
                    &phone_ids(),
                    clone.writer.id(),
                    &clone_writers,
                    MAX_EXCHANGE_BYTES,
                )
                .await
                .expect("exchange after re-seal"),
        );
        assert_eq!(exchanged.problem, None);
        assert_eq!(clone.stored().await, hub_replica.stored().await);

        hub_replica.apply().await;
        assert!(hub_replica.has_note("note-original").await);
        assert!(hub_replica.has_note("note-clone").await);
        hub_replica.close().await;
        original.close().await;
        clone.close().await;
    });
}

/// Exchanges once and applies on both replicas.
async fn converge(hub: &mut Replica, phone: &mut Replica, transport: &DirectTransport) {
    done(
        phone
            .exchange(transport, &phone_ids())
            .await
            .expect("exchange"),
    );
    hub.apply().await;
    phone.apply().await;
}

async fn note_json(replica: &Replica, id: &str) -> serde_json::Value {
    let note = ganbaru_quick_notes::load_note_from_pool(&replica.pool, id)
        .await
        .expect("load note");
    serde_json::to_value(note).expect("serialize note")
}

async fn recovery_titles(replica: &Replica) -> Vec<(String, String)> {
    Engine::vault()
        .recovery_entries(&mut *acquire(&replica.pool).await, &replica.ctx)
        .await
        .expect("recovery entries")
        .into_iter()
        .map(|entry| (entry.row_key, entry.presentation.title))
        .collect()
}

#[test]
fn a_resolved_title_conflict_closes_on_both_replicas() {
    block_on(async {
        let mut hub_replica = Replica::new("conflict-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("conflict-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica.pool);
        hub_replica.seal_note("note-1").await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        hub_replica
            .edit(
                "UPDATE quick_notes SET title = 'Hub title' WHERE id = ?",
                "note-1",
            )
            .await;
        phone
            .edit(
                "UPDATE quick_notes SET title = 'Phone title' WHERE id = ?",
                "note-1",
            )
            .await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        let hub_note = note_json(&hub_replica, "note-1").await;
        let phone_note = note_json(&phone, "note-1").await;
        assert_eq!(hub_note["hasConflict"], true);
        assert_eq!(phone_note["hasConflict"], true);
        assert_eq!(hub_note["title"], phone_note["title"]);

        let conflict = ganbaru_quick_notes::conflict_from_pool(
            &phone.pool,
            VAULT_ID,
            &ganbaru_sync::DeviceNames::new(PHONE_DEVICE, HashMap::new()),
            "note-1",
        )
        .await
        .expect("read conflict");
        let conflict = serde_json::to_value(conflict).expect("serialize conflict");
        let groups = conflict["groups"].as_array().expect("groups");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["field"], "title");
        let versions = groups[0]["versions"].as_array().expect("versions");
        assert_eq!(versions.len(), 2);
        let displayed: Vec<_> = versions
            .iter()
            .filter(|version| version["displayed"] == true)
            .collect();
        assert_eq!(displayed.len(), 1);
        assert_eq!(displayed[0]["title"], phone_note["title"]);
        let own = versions
            .iter()
            .find(|version| version["device"]["ownDevice"] == true)
            .expect("own version");
        assert_eq!(own["title"], "Phone title");
        let other = versions
            .iter()
            .find(|version| version["displayed"] == false)
            .expect("hidden version");

        let resolution = serde_json::from_value(serde_json::json!({
            "id": "note-1",
            "field": "title",
            "choice": { "kind": "version", "version": other["version"] },
        }))
        .expect("resolution");
        let resolved =
            ganbaru_quick_notes::resolve_from_pool(&phone.pool, VAULT_ID, resolution, NOW_MS + 1)
                .await
                .expect("resolve conflict");
        let resolved = serde_json::to_value(resolved).expect("serialize resolution");
        assert_eq!(resolved["note"]["title"], other["title"]);
        assert_eq!(resolved["note"]["hasConflict"], false);
        assert_eq!(resolved["copy"], serde_json::Value::Null);

        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            let note = note_json(replica, "note-1").await;
            assert_eq!(note["title"], other["title"]);
            assert_eq!(note["hasConflict"], false);
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn restoring_an_edit_that_outlived_a_deletion_closes_the_offer_everywhere() {
    block_on(async {
        let mut hub_replica = Replica::new("recovery-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("recovery-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica.pool);
        hub_replica.seal_note("note-2").await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        hub_replica
            .edit("DELETE FROM quick_notes WHERE id = ?", "note-2")
            .await;
        phone
            .edit(
                "UPDATE quick_notes SET title = 'Kept edit' WHERE id = ?",
                "note-2",
            )
            .await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        let offer = vec![("note-2".to_string(), "Kept edit".to_string())];
        for replica in [&hub_replica, &phone] {
            assert!(!replica.has_note("note-2").await);
            assert_eq!(recovery_titles(replica).await, offer);
        }

        let restored = crate::recovery::close(
            &Engine::vault(),
            &phone.pool,
            &phone.ctx,
            &mut phone.writer,
            &RecoveryChoice {
                table: NOTES_TABLE,
                row_key: "note-2".to_string(),
                action: RecoveryAction::Restore,
            },
            NOW_MS + 1,
        )
        .await
        .expect("restore")
        .expect("restored note id");
        assert_eq!(note_json(&phone, &restored).await["title"], "Kept edit");

        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert!(replica.has_note(&restored).await);
            assert!(!replica.has_note("note-2").await);
            assert!(recovery_titles(replica).await.is_empty());
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn discarding_a_recovery_offer_keeps_the_deletion() {
    block_on(async {
        let mut hub_replica = Replica::new("discard-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("discard-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica.pool);
        hub_replica.seal_note("note-3").await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        phone
            .edit("DELETE FROM quick_notes WHERE id = ?", "note-3")
            .await;
        hub_replica
            .edit(
                "UPDATE quick_notes SET title = 'Late edit' WHERE id = ?",
                "note-3",
            )
            .await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        assert_eq!(recovery_titles(&hub_replica).await.len(), 1);

        let restored = crate::recovery::close(
            &Engine::vault(),
            &hub_replica.pool,
            &hub_replica.ctx,
            &mut hub_replica.writer,
            &RecoveryChoice {
                table: NOTES_TABLE,
                row_key: "note-3".to_string(),
                action: RecoveryAction::Discard,
            },
            NOW_MS + 1,
        )
        .await
        .expect("discard");
        assert_eq!(restored, None);

        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert!(!replica.has_note("note-3").await);
            assert!(recovery_titles(replica).await.is_empty());
        }
        hub_replica.close().await;
        phone.close().await;
    });
}
