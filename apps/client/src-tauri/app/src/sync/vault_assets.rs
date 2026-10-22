//! The managed asset files of the active vault as the sync service and the hub move them.
//! Reads and staging use the vault root resolved when the store opens. Each commit holds the
//! replicated asset permit of the domain that owns the asset class for that one file, so a vault
//! handoff fences it, and tells the frontend which tables reference the arrived asset so views
//! waiting for it load again.

use super::assets::{AssetStore, AssetStoreError, ContentAddressedStore, StagedAsset};
use super::host::SyncHost;
use ganbaru_quick_notes::images;
use ganbaru_sync::Engine;
use ganbaru_sync::assets::AssetRef;
use ganbaru_sync::manifest::vault::quick_notes::IMAGES_CLASS;
use std::path::PathBuf;

pub(super) struct ActiveVaultAssets<H: SyncHost> {
    host: H,
    root: PathBuf,
    files: ContentAddressedStore,
    engine: Engine,
}

impl<H: SyncHost> ActiveVaultAssets<H> {
    /// The asset files of the active vault, which must be `vault_id`, with the content checks
    /// of every class that has one.
    pub(super) fn open(host: &H, vault_id: &str) -> Result<Self, String> {
        if host.active_vault_id()? != vault_id {
            return Err("the active vault changed during sync".to_string());
        }
        let root = host.active_vault_path()?;
        Ok(Self {
            host: host.clone(),
            files: ContentAddressedStore::new(root.clone())
                .with_check(&IMAGES_CLASS, images::check_synced_image),
            root,
            engine: Engine::vault(),
        })
    }
}

impl<H: SyncHost> AssetStore for ActiveVaultAssets<H> {
    fn holds(&self, asset: &AssetRef) -> Result<bool, AssetStoreError> {
        self.files.holds(asset)
    }

    fn read_chunk(
        &self,
        asset: &AssetRef,
        offset: u32,
        max_len: usize,
    ) -> Result<Vec<u8>, AssetStoreError> {
        self.files.read_chunk(asset, offset, max_len)
    }

    fn stage(&self, asset: &AssetRef) -> Result<StagedAsset, AssetStoreError> {
        self.files.stage(asset)
    }

    fn commit(&self, staged: StagedAsset) -> Result<(), AssetStoreError> {
        let class_name = staged.asset().class().name;
        let Some((domain, class)) = self.engine.manifest().asset_class(class_name) else {
            return Err(AssetStoreError::Failed(format!(
                "asset class {class_name} is not registered"
            )));
        };
        let root = self
            .host
            .replicated_asset_path(domain.name)
            .map_err(AssetStoreError::Failed)?;
        if *root != *self.root {
            return Err(AssetStoreError::Failed(
                "the active vault changed during sync".to_string(),
            ));
        }
        self.files.commit(staged)?;
        drop(root);
        let tables = self.engine.asset_tables(class);
        self.host
            .emit_applied(tables.into_iter().map(str::to_string).collect());
        Ok(())
    }
}
