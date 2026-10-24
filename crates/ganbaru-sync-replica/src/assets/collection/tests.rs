//! The sweep keeps asset files that only a `Local` row names, and files a row names once it
//! commits while the sweep lists the directories.

use super::{
    AssetSweep, InFlightAssets, collect_unreferenced_assets, kept_digests, remove_candidates,
    unreferenced_candidates,
};
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
            digest: LocalDigest::AssetId {
                classes: &[PINS_CLASS.name],
            },
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
    /// A root of its own for the test named `label`, since tests of one process run at once.
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ganbaru-sync-local-assets-{label}-{}",
            std::process::id()
        ));
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

/// A vault of the pins domain holding one asset file older than the grace that no row names.
struct Vault {
    root: Root,
    engine: Engine,
    pool: SqlitePool,
    digest: String,
    file: PathBuf,
}

impl Vault {
    async fn new(label: &str) -> Self {
        let root = Root::new(label);
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
        Self {
            root,
            engine,
            pool,
            digest,
            file,
        }
    }

    async fn pin(&self) {
        sqlx::query("INSERT INTO pins (id, asset_id) VALUES ('pin', ?)")
            .bind(format!("{}:{}", PINS_CLASS.name, self.digest))
            .execute(&self.pool)
            .await
            .expect("pin");
    }

    fn sweep<'a>(&'a self, in_flight: &'a Arc<InFlightAssets>) -> AssetSweep<'a> {
        AssetSweep {
            engine: &self.engine,
            pool: &self.pool,
            vault_root: self.root.path(),
            in_flight,
            grace: GRACE,
            now: SystemTime::now(),
        }
    }
}

#[test]
fn a_row_that_commits_while_the_sweep_lists_keeps_its_file() {
    block_on(async {
        let vault = Vault::new("listing").await;
        let in_flight = Arc::new(InFlightAssets::default());
        let sweep = vault.sweep(&in_flight);
        let before = kept_digests(sweep).await.unwrap();
        let candidates =
            unreferenced_candidates(vault.root.path(), &[&PINS_CLASS], &before, GRACE, sweep.now)
                .unwrap();
        assert_eq!(candidates.len(), 1, "the unnamed aged file is a candidate");

        vault.pin().await;
        let after = kept_digests(sweep).await.unwrap();

        assert_eq!(
            remove_candidates(candidates.clone(), &after, &in_flight, GRACE),
            Ok(0)
        );
        assert!(vault.file.exists(), "the second read keeps the file");
        assert_eq!(
            remove_candidates(candidates, &before, &in_flight, GRACE),
            Ok(1),
            "a file neither read keeps goes"
        );
    });
}

#[test]
fn a_file_only_a_local_row_names_stays_until_the_row_goes() {
    block_on(async {
        let vault = Vault::new("local-row").await;
        let (engine, pool, root, file) = (&vault.engine, &vault.pool, &vault.root, &vault.file);
        vault.pin().await;

        assert_eq!(sweep(engine, pool, root.path()).await, Ok(0));
        assert!(file.exists(), "a file a Local row names stays");

        sqlx::query("DELETE FROM pins")
            .execute(pool)
            .await
            .expect("unpin");
        assert_eq!(sweep(engine, pool, root.path()).await, Ok(1));
        assert!(!file.exists(), "the file goes once no row names it");
    });
}
