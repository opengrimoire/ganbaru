//! The sweep keeps asset files that only a `Local` row names.

use super::{AssetSweep, InFlightAssets, collect_unreferenced_assets};
use crate::backup::ArchiveGate;
use ganbaru_sync::assets::{AssetClass, LocalAssetColumn, LocalDigest};
use ganbaru_sync::manifest::{DeviceTable, DomainManifest, Manifest, TableIdRange};
use ganbaru_sync::{Adapters, Engine, Hooks};
use sha2::{Digest as _, Sha256};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, SystemTime};

const GRACE: Duration = Duration::from_secs(60 * 60);

static PINS_CLASS: AssetClass = AssetClass {
    name: "test/pins",
    max_bytes: 1024,
};

/// A domain whose only reference to its class is a `Local` table, as a device-only cover or
/// icon choice is.
static PINS_DOMAIN: DomainManifest = DomainManifest {
    name: "pins",
    table_ids: TableIdRange { first: 1, last: 16 },
    tables: &[],
    derived: &[],
    local: &[DeviceTable {
        name: "pins",
        triggers: &[],
        assets: &[LocalAssetColumn {
            column: "asset_id",
            json_path: None,
            digest: LocalDigest::AssetId,
        }],
    }],
    engine: &[],
    replica_writable: true,
    assets: &[PINS_CLASS],
};

static PINS_DOMAINS: [&DomainManifest; 1] = [&PINS_DOMAIN];

static PINS_MANIFEST: LazyLock<Manifest> =
    LazyLock::new(|| Manifest::from_domains(1, &PINS_DOMAINS));

/// A vault root under the system temporary directory, removed on drop.
struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("ganbaru-sync-local-assets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create vault root");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime")
        .block_on(future)
}

async fn sweep(engine: &Engine, pool: &SqlitePool, root: &Path) -> Result<usize, String> {
    collect_unreferenced_assets(
        &ArchiveGate::default(),
        AssetSweep {
            engine,
            pool,
            vault_root: root,
            in_flight: &Arc::new(InFlightAssets::default()),
            grace: GRACE,
            now: SystemTime::now(),
        },
        |_| Some(()),
    )
    .await
}

#[test]
fn a_file_only_a_local_row_names_stays_until_the_row_goes() {
    block_on(async {
        let root = Root::new();
        let engine = Engine::with(&PINS_MANIFEST, Adapters::default(), Hooks::default())
            .expect("pins manifest");
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("pool");
        sqlx::raw_sql("CREATE TABLE pins (id TEXT PRIMARY KEY, asset_id TEXT)")
            .execute(&pool)
            .await
            .expect("schema");
        let digest: String = Sha256::digest(b"pinned")
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let directory = PINS_CLASS.directory(root.path());
        std::fs::create_dir_all(&directory).expect("class directory");
        let file = directory.join(format!("{digest}.png"));
        std::fs::write(&file, b"pinned").expect("write asset");
        std::fs::File::open(&file)
            .and_then(|handle| handle.set_modified(SystemTime::now() - GRACE * 2))
            .expect("age asset");
        sqlx::query("INSERT INTO pins (id, asset_id) VALUES ('pin', ?)")
            .bind(format!("{}:{digest}", PINS_CLASS.name))
            .execute(&pool)
            .await
            .expect("pin");

        assert_eq!(sweep(&engine, &pool, root.path()).await, Ok(0));
        assert!(file.exists(), "a file a Local row names stays");

        sqlx::query("DELETE FROM pins")
            .execute(&pool)
            .await
            .expect("unpin");
        assert_eq!(sweep(&engine, &pool, root.path()).await, Ok(1));
        assert!(!file.exists(), "the file goes once no row names it");
    });
}
