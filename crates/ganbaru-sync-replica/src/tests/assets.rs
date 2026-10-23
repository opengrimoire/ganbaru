use super::{TestDirectory, acquire, add_note, block_on, create_vault};
use crate::assets::{
    AssetStore, AssetStoreError, AssetSweep, ContentAddressedStore, InFlightAssets,
    STALE_STAGING_AGE, collect_unreferenced_assets,
};
use crate::backup::ArchiveGate;
use crate::carry_forward::carry_assets;
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_handoff::protocol::SYNC_ASSET_CHUNK_BYTES;
use ganbaru_sync::Engine;
use ganbaru_sync::assets::{AssetClass, AssetRef, MAX_ASSET_BYTES};
use ganbaru_sync::domains::quick_notes::{NoteImage, encode_images};
use ganbaru_sync::manifest::vault::preferences::PROFILE_CLASS;
use ganbaru_sync::manifest::vault::quick_notes::{IMAGES_CLASS, NOTES_TABLE, note_group};
use ganbaru_sync_contracts::{Field, Value};
use sha2::{Digest as _, Sha256};
use sqlx::SqlitePool;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// A class at the largest asset bound, larger than the bound of every class the vault registers.
static LARGE_CLASS: AssetClass = AssetClass {
    name: "test/large",
    max_bytes: MAX_ASSET_BYTES,
};

/// Deterministic bytes that vary with `seed`.
fn bytes(len: usize, seed: u8) -> Vec<u8> {
    let mut state = u32::from(seed).wrapping_mul(2_654_435_761).wrapping_add(1);
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state.to_le_bytes()[0]
        })
        .collect()
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn reference(class: &'static AssetClass, bytes: &[u8]) -> AssetRef {
    AssetRef::new(
        class,
        digest(bytes),
        "application/octet-stream".to_string(),
        u32::try_from(bytes.len()).expect("asset size"),
    )
    .expect("asset reference")
}

/// Names of every entry in the directory of `class`, temporary files included.
fn entries(root: &Path, class: &AssetClass) -> Vec<String> {
    let mut names: Vec<String> = match std::fs::read_dir(class.directory(root)) {
        Ok(entries) => entries
            .map(|entry| {
                entry
                    .expect("asset entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("list assets: {error}"),
    };
    names.sort();
    names
}

/// Streams `bytes` into the store chunk by chunk and commits them as `asset`.
fn receive(
    store: &ContentAddressedStore,
    asset: &AssetRef,
    bytes: &[u8],
) -> Result<(), AssetStoreError> {
    let mut staged = store.stage(asset)?;
    for chunk in bytes.chunks(SYNC_ASSET_CHUNK_BYTES) {
        staged.write(chunk)?;
    }
    store.commit(staged)
}

#[test]
fn assets_larger_than_the_old_bound_stream_into_place() {
    let directory = TestDirectory::new("assets-stream");
    let store = ContentAddressedStore::new(directory.path().to_path_buf());
    let large = bytes(33 * 1024 * 1024 + 7, 1);
    let asset = reference(&LARGE_CLASS, &large);

    let mut staged = store.stage(&asset).expect("stage");
    for chunk in large.chunks(SYNC_ASSET_CHUNK_BYTES).take(3) {
        staged.write(chunk).expect("write chunk");
    }
    assert!(!staged.is_complete());
    assert!(!store.holds(&asset).expect("holds"));
    assert_eq!(entries(directory.path(), &LARGE_CLASS).len(), 1);
    drop(staged);
    assert_eq!(
        entries(directory.path(), &LARGE_CLASS),
        Vec::<String>::new()
    );

    receive(&store, &asset, &large).expect("receive");
    assert!(store.holds(&asset).expect("holds"));
    assert_eq!(
        std::fs::read(asset.path(directory.path())).expect("read"),
        large
    );
    assert_eq!(
        entries(directory.path(), &LARGE_CLASS),
        vec![asset.file_name()]
    );
    let tail = store
        .read_chunk(&asset, asset.byte_size() - 7, SYNC_ASSET_CHUNK_BYTES)
        .expect("read tail");
    assert_eq!(tail, large[large.len() - 7..]);
}

#[test]
fn assets_whose_bytes_do_not_match_leave_nothing_behind() {
    let directory = TestDirectory::new("assets-mismatch");
    let store = ContentAddressedStore::new(directory.path().to_path_buf());
    let original = bytes(SYNC_ASSET_CHUNK_BYTES * 2 + 3, 2);
    let asset = reference(&LARGE_CLASS, &original);

    let mut corrupt = original.clone();
    corrupt[SYNC_ASSET_CHUNK_BYTES] ^= 0xff;
    assert!(matches!(
        receive(&store, &asset, &corrupt),
        Err(AssetStoreError::Mismatch(_))
    ));
    let mut longer = original.clone();
    longer.push(0);
    assert!(matches!(
        receive(&store, &asset, &longer),
        Err(AssetStoreError::Mismatch(_))
    ));
    let mut staged = store.stage(&asset).expect("stage");
    assert!(matches!(
        staged.write(&[]),
        Err(AssetStoreError::Mismatch(_))
    ));
    drop(staged);
    let short = store.stage(&asset).expect("stage");
    assert!(matches!(
        store.commit(short),
        Err(AssetStoreError::Mismatch(_))
    ));

    assert!(!store.holds(&asset).expect("holds"));
    assert_eq!(
        entries(directory.path(), &LARGE_CLASS),
        Vec::<String>::new()
    );
}

#[test]
fn assets_already_held_are_kept_and_class_checks_refuse_the_rest() {
    let directory = TestDirectory::new("assets-held");
    let store = ContentAddressedStore::new(directory.path().to_path_buf()).with_check(
        &LARGE_CLASS,
        |_, path| {
            let len = std::fs::metadata(path)
                .map_err(|error| error.to_string())?
                .len();
            if len % 2 == 0 {
                Ok(())
            } else {
                Err("the test class holds only even sizes".to_string())
            }
        },
    );
    let even = bytes(64, 3);
    let asset = reference(&LARGE_CLASS, &even);
    receive(&store, &asset, &even).expect("receive");
    let modified = std::fs::metadata(asset.path(directory.path()))
        .expect("metadata")
        .modified()
        .expect("modified");
    receive(&store, &asset, &even).expect("receive held asset");
    assert_eq!(
        std::fs::metadata(asset.path(directory.path()))
            .expect("metadata")
            .modified()
            .expect("modified"),
        modified
    );

    let odd = bytes(65, 4);
    let refused = reference(&LARGE_CLASS, &odd);
    assert_eq!(
        receive(&store, &refused, &odd),
        Err(AssetStoreError::Mismatch(
            "the test class holds only even sizes".to_string()
        ))
    );
    assert_eq!(
        entries(directory.path(), &LARGE_CLASS),
        vec![asset.file_name()]
    );
}

#[test]
fn a_damaged_held_file_of_the_same_size_is_replaced_by_verified_bytes() {
    let directory = TestDirectory::new("assets-damaged");
    let store = ContentAddressedStore::new(directory.path().to_path_buf());
    let original = bytes(SYNC_ASSET_CHUNK_BYTES + 5, 6);
    let asset = reference(&LARGE_CLASS, &original);
    let mut damaged = original.clone();
    damaged[3] ^= 0xff;
    store_file(directory.path(), &LARGE_CLASS, &asset.file_name(), &damaged);
    // The cheap check sees only the size, so the damaged file still counts as held.
    assert!(store.holds(&asset).expect("holds"));

    receive(&store, &asset, &original).expect("receive");
    assert_eq!(
        std::fs::read(asset.path(directory.path())).expect("read"),
        original
    );
    assert_eq!(
        entries(directory.path(), &LARGE_CLASS),
        vec![asset.file_name()]
    );
}

/// Sets the modification time of the entry at `path`.
fn age(path: &Path, modified: SystemTime) {
    std::fs::File::open(path)
        .and_then(|file| file.set_modified(modified))
        .expect("age entry");
}

#[test]
fn stale_staged_files_are_swept_and_every_other_entry_stays() {
    let directory = TestDirectory::new("assets-sweep");
    let store = ContentAddressedStore::new(directory.path().to_path_buf());
    let held = bytes(40, 10);
    let asset = reference(&LARGE_CLASS, &held);
    receive(&store, &asset, &held).expect("receive");
    let class_directory = LARGE_CLASS.directory(directory.path());

    let staged = |nonce: char| {
        format!(
            ".{}.{}.tmp",
            asset.file_name(),
            nonce.to_string().repeat(32)
        )
    };
    let abandoned = staged('a');
    let fresh = staged('b');
    let lookalike = format!("{}.{}.tmp", asset.file_name(), "c".repeat(32));
    let staged_directory = format!(".dir.png.{}.tmp", "d".repeat(32));
    for name in [&abandoned, &fresh, &lookalike, &"notes.tmp".to_string()] {
        store_file(directory.path(), &LARGE_CLASS, name, &held[..8]);
    }
    std::fs::create_dir(class_directory.join(&staged_directory)).expect("create directory");
    let old = SystemTime::now() - STALE_STAGING_AGE - Duration::from_secs(1);
    for name in [
        &abandoned,
        &lookalike,
        &"notes.tmp".to_string(),
        &staged_directory,
    ] {
        age(&class_directory.join(name), old);
    }

    assert_eq!(
        store.remove_stale_staging([&LARGE_CLASS, &IMAGES_CLASS], STALE_STAGING_AGE),
        Ok(1)
    );
    let mut expected = vec![
        asset.file_name(),
        fresh,
        lookalike,
        "notes.tmp".to_string(),
        staged_directory,
    ];
    expected.sort();
    assert_eq!(entries(directory.path(), &LARGE_CLASS), expected);
    assert_eq!(
        store.remove_stale_staging([&LARGE_CLASS], STALE_STAGING_AGE),
        Ok(0)
    );
}

/// Writes `bytes` as an asset file of `class` named by their digest.
fn store_file(root: &Path, class: &AssetClass, name: &str, bytes: &[u8]) {
    let directory = class.directory(root);
    std::fs::create_dir_all(&directory).expect("create class directory");
    std::fs::write(directory.join(name), bytes).expect("write asset");
}

#[test]
fn replacements_keep_the_verified_assets_only_this_device_holds() {
    let current = TestDirectory::new("carry-current");
    let replacement = TestDirectory::new("carry-replacement");
    let manifest = Engine::vault().manifest();
    let shared = bytes(100, 5);
    let local = bytes(100, 6);
    let damaged = bytes(100, 7);
    let oversized = bytes(IMAGES_CLASS.max_bytes as usize + 1, 8);
    for (contents, written) in [
        (&shared, &shared),
        (&local, &local),
        (&damaged, &bytes(100, 9)),
        (&oversized, &oversized),
    ] {
        let name = format!("{}.png", digest(contents));
        store_file(current.path(), &IMAGES_CLASS, &name, written);
    }
    store_file(current.path(), &IMAGES_CLASS, "notes.txt", b"x");
    store_file(
        current.path(),
        &IMAGES_CLASS,
        &format!("{}.jpeg", digest(&shared)),
        &shared,
    );
    let shared_name = format!("{}.png", digest(&shared));
    store_file(replacement.path(), &IMAGES_CLASS, &shared_name, &shared);

    assert_eq!(
        carry_assets(manifest, current.path(), replacement.path()),
        Ok(1)
    );
    let local_name = format!("{}.png", digest(&local));
    assert_eq!(entries(replacement.path(), &IMAGES_CLASS), {
        let mut expected = vec![shared_name, local_name.clone()];
        expected.sort();
        expected
    });
    assert_eq!(
        std::fs::read(IMAGES_CLASS.directory(replacement.path()).join(&local_name))
            .expect("read carried asset"),
        local
    );
    assert_eq!(
        carry_assets(manifest, current.path(), replacement.path()),
        Ok(0)
    );

    let empty = TestDirectory::new("carry-empty");
    assert_eq!(
        carry_assets(manifest, empty.path(), replacement.path()),
        Ok(0)
    );
}

/// Grace the collection tests sweep with; files they age are older than it.
const SWEEP_GRACE: Duration = Duration::from_secs(60 * 60);

/// A vault database under a test directory, whose root holds the asset class directories.
struct SweepVault {
    directory: TestDirectory,
    _registry: DatabasePoolRegistry,
    pool: SqlitePool,
}

impl SweepVault {
    async fn new(label: &str) -> Self {
        let directory = TestDirectory::new(label);
        let registry = DatabasePoolRegistry::default();
        let pool = create_vault(&registry, &directory.path().join("ganbaru.sqlite")).await;
        Self {
            directory,
            _registry: registry,
            pool,
        }
    }

    fn root(&self) -> &Path {
        self.directory.path()
    }

    /// Writes `contents` as a PNG asset of `class`, last modified before the sweep grace, and
    /// returns its file name.
    fn aged_file(&self, class: &AssetClass, contents: &[u8]) -> String {
        let name = format!("{}.png", digest(contents));
        self.aged_entry(class, &name, contents);
        name
    }

    /// Writes an entry named `name` in the directory of `class`, last modified before the
    /// sweep grace.
    fn aged_entry(&self, class: &AssetClass, name: &str, contents: &[u8]) {
        store_file(self.root(), class, name, contents);
        age(
            &class.directory(self.root()).join(name),
            SystemTime::now() - SWEEP_GRACE * 2,
        );
    }

    /// Adds a note whose image list references `contents`.
    async fn reference_note_image(&self, note_id: &str, contents: &[u8]) {
        let mut conn = acquire(&self.pool).await;
        add_note(&mut conn, note_id).await;
        sqlx::query(
            "INSERT INTO quick_note_images
                 (note_id, sort_order, sha256, content_type, byte_size, width, height)
             VALUES (?, 0, ?, 'image/png', ?, 4, 4)",
        )
        .bind(note_id)
        .bind(digest(contents))
        .bind(i64::try_from(contents.len()).expect("image size"))
        .execute(&mut *conn)
        .await
        .expect("insert note image");
    }

    /// Keeps a retained version of a deleted note whose image list references `contents`, as
    /// an open conflict or recovery offer does.
    async fn retain_note_image(&self, contents: &[u8]) {
        let image = NoteImage {
            sha256: digest(contents),
            content_type: "image/png".to_string(),
            byte_size: u32::try_from(contents.len()).expect("image size"),
            width: 4,
            height: 4,
        };
        let encoded = encode_images(&[image]).expect("encode images");
        let value = Value::new(vec![Field::Blob(encoded)]).expect("image value");
        sqlx::query(
            "INSERT INTO sync_rows (table_id, row_key, state) VALUES (?, 'gone', 'tombstoned')",
        )
        .bind(i64::from(NOTES_TABLE.0))
        .execute(&self.pool)
        .await
        .expect("insert tombstone");
        sqlx::query(
            "INSERT INTO sync_register_versions
                 (table_id, row_key, group_id, writer_id, seq, clock, value_hash, value)
             VALUES (?, 'gone', ?, zeroblob(16), 1, 0, zeroblob(32), ?)",
        )
        .bind(i64::from(NOTES_TABLE.0))
        .bind(i64::from(note_group::IMAGES.get()))
        .bind(value.encode())
        .execute(&self.pool)
        .await
        .expect("insert retained version");
    }

    /// Sets the profile picture preference to `contents`.
    async fn set_profile_image(&self, contents: &[u8]) {
        let value = format!(
            r#"{{"path":"profile/{}.png","byteSize":{}}}"#,
            digest(contents),
            contents.len()
        );
        sqlx::query(
            "INSERT INTO vault_preferences (key, value, updated_at)
             VALUES ('profile.image', ?, '2026-01-01T00:00:00.000Z')
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        )
        .bind(value)
        .execute(&self.pool)
        .await
        .expect("set profile image");
    }

    /// Runs one sweep over the classes `admit` accepts.
    async fn sweep(
        &self,
        in_flight: &Arc<InFlightAssets>,
        now: SystemTime,
        admit: impl FnMut(&'static AssetClass) -> Option<()>,
    ) -> Result<usize, String> {
        let engine = Engine::vault();
        collect_unreferenced_assets(
            &ArchiveGate::default(),
            AssetSweep {
                engine: &engine,
                pool: &self.pool,
                vault_root: self.root(),
                in_flight,
                grace: SWEEP_GRACE,
                now,
            },
            admit,
        )
        .await
    }
}

fn admit_all(_: &'static AssetClass) -> Option<()> {
    Some(())
}

#[test]
fn assets_sweep_removes_unreferenced_files_of_every_class_and_keeps_referenced_ones() {
    block_on(async {
        let vault = SweepVault::new("assets-collect").await;
        let [live, retained, orphan, profile, replaced, unregistered] =
            [20, 21, 22, 23, 24, 25].map(|seed| bytes(64, seed));
        let live_name = vault.aged_file(&IMAGES_CLASS, &live);
        let retained_name = vault.aged_file(&IMAGES_CLASS, &retained);
        let orphan_name = vault.aged_file(&IMAGES_CLASS, &orphan);
        let profile_name = vault.aged_file(&PROFILE_CLASS, &profile);
        vault.aged_file(&PROFILE_CLASS, &replaced);
        let unregistered_name = vault.aged_file(&LARGE_CLASS, &unregistered);
        let staged = format!(".{orphan_name}.{}.tmp", "a".repeat(32));
        for name in [staged.as_str(), "notes.txt"] {
            vault.aged_entry(&IMAGES_CLASS, name, b"partial");
        }
        vault.reference_note_image("note", &live).await;
        vault.retain_note_image(&retained).await;
        vault.set_profile_image(&replaced).await;
        vault.set_profile_image(&profile).await;

        let in_flight = Arc::new(InFlightAssets::default());
        assert_eq!(
            vault.sweep(&in_flight, SystemTime::now(), admit_all).await,
            Ok(2),
            "the orphaned note image and the replaced profile picture go"
        );
        let mut images = vec![live_name, retained_name, staged, "notes.txt".to_string()];
        images.sort();
        assert_eq!(entries(vault.root(), &IMAGES_CLASS), images);
        assert_eq!(entries(vault.root(), &PROFILE_CLASS), vec![profile_name]);
        assert_eq!(
            entries(vault.root(), &LARGE_CLASS),
            vec![unregistered_name],
            "a directory no manifest class names stays"
        );
        assert_eq!(
            vault.sweep(&in_flight, SystemTime::now(), admit_all).await,
            Ok(0)
        );
    });
}

#[test]
fn assets_sweep_spares_recent_touched_and_refused_files() {
    block_on(async {
        let vault = SweepVault::new("assets-spare").await;
        let [touched, recent, refused] = [30, 31, 32].map(|seed| bytes(64, seed));
        let touched_name = vault.aged_file(&IMAGES_CLASS, &touched);
        let recent_name = format!("{}.png", digest(&recent));
        store_file(vault.root(), &IMAGES_CLASS, &recent_name, &recent);
        let refused_name = vault.aged_file(&PROFILE_CLASS, &refused);
        let in_flight = Arc::new(InFlightAssets::default());
        in_flight.touch(&reference(&IMAGES_CLASS, &touched));
        let refuse_profiles =
            |class: &'static AssetClass| (class.name != PROFILE_CLASS.name).then_some(());

        let now = SystemTime::now();
        assert_eq!(vault.sweep(&in_flight, now, refuse_profiles).await, Ok(0));
        let mut images = vec![touched_name, recent_name];
        images.sort();
        assert_eq!(entries(vault.root(), &IMAGES_CLASS), images);
        assert_eq!(entries(vault.root(), &PROFILE_CLASS), vec![refused_name]);

        assert_eq!(
            vault.sweep(&in_flight, now, admit_all).await,
            Ok(1),
            "an admitted class loses its aged orphan"
        );
        assert!(entries(vault.root(), &PROFILE_CLASS).is_empty());

        let untouched = Arc::new(InFlightAssets::default());
        assert_eq!(
            vault
                .sweep(&untouched, now + SWEEP_GRACE * 2, admit_all)
                .await,
            Ok(2),
            "past the grace, files no transfer touched go"
        );
        assert!(entries(vault.root(), &IMAGES_CLASS).is_empty());
    });
}

#[test]
fn assets_sweep_waits_for_an_archive_in_progress() {
    block_on(async {
        let vault = SweepVault::new("assets-gate").await;
        vault.aged_file(&IMAGES_CLASS, &bytes(64, 40));
        let gate = ArchiveGate::default();
        let in_flight = Arc::new(InFlightAssets::default());
        let engine = Engine::vault();
        let sweep = || {
            collect_unreferenced_assets(
                &gate,
                AssetSweep {
                    engine: &engine,
                    pool: &vault.pool,
                    vault_root: vault.root(),
                    in_flight: &in_flight,
                    grace: SWEEP_GRACE,
                    now: SystemTime::now(),
                },
                admit_all,
            )
        };

        let archive = gate.lock().await;
        assert!(
            tokio::time::timeout(Duration::from_millis(200), sweep())
                .await
                .is_err(),
            "the sweep waits while an archive holds the gate"
        );
        assert_eq!(entries(vault.root(), &IMAGES_CLASS).len(), 1);
        drop(archive);
        assert_eq!(sweep().await, Ok(1));
    });
}
