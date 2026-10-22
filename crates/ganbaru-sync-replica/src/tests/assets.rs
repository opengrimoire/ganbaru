use super::TestDirectory;
use crate::assets::{AssetStore, AssetStoreError, ContentAddressedStore};
use crate::carry_forward::carry_assets;
use ganbaru_handoff::protocol::SYNC_ASSET_CHUNK_BYTES;
use ganbaru_sync::Engine;
use ganbaru_sync::assets::{AssetClass, AssetRef, MAX_ASSET_BYTES};
use ganbaru_sync::manifest::vault::quick_notes::IMAGES_CLASS;
use sha2::{Digest as _, Sha256};
use std::path::Path;

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
