//! The active vault's Quick note image files as the sync service and the hub move them. Reads
//! use the vault root resolved when the store opens. Each write holds the replicated asset
//! permit for that one file, so a vault handoff fences it, and tells the frontend that an image
//! arrived so tiles waiting for it load again.

use super::status::emit_applied;
use crate::vault;
use ganbaru_quick_notes::images::{self, QuickNoteImageError, ReferencedImage};
use ganbaru_sync::manifest::vault::quick_notes::DOMAIN_NAME as QUICK_NOTES_DOMAIN;
use ganbaru_sync_replica::assets::ImageStore;
use std::path::PathBuf;
use tauri::{AppHandle, Runtime};

/// The table name the frontend reloads Quick note images for.
const IMAGES_TABLE: &str = "quick_note_images";

pub(super) struct ActiveVaultImages<R: Runtime> {
    app: AppHandle<R>,
    root: PathBuf,
}

impl<R: Runtime> ActiveVaultImages<R> {
    /// The image files of the active vault, which must be `vault_id`.
    pub(super) fn open(app: &AppHandle<R>, vault_id: &str) -> Result<Self, String> {
        if vault::active_vault_id(app)? != vault_id {
            return Err("the active vault changed during sync".to_string());
        }
        Ok(Self {
            app: app.clone(),
            root: vault::active_vault_path(app)?,
        })
    }
}

impl<R: Runtime> ImageStore for ActiveVaultImages<R> {
    fn holds(&self, image: &ReferencedImage) -> Result<bool, QuickNoteImageError> {
        images::holds_image(&self.root, image)
    }

    fn read_chunk(
        &self,
        image: &ReferencedImage,
        offset: u32,
        max_len: usize,
    ) -> Result<Vec<u8>, QuickNoteImageError> {
        images::read_image_chunk(&self.root, image, offset, max_len)
    }

    fn store(&self, image: &ReferencedImage, bytes: &[u8]) -> Result<(), QuickNoteImageError> {
        let root = vault::active_replicated_asset_path(&self.app, QUICK_NOTES_DOMAIN)?;
        if *root != *self.root {
            return Err(QuickNoteImageError::Failed(
                "the active vault changed during sync".to_string(),
            ));
        }
        images::store_image(&root, image, bytes)?;
        drop(root);
        emit_applied(&self.app, vec![IMAGES_TABLE.to_string()]);
        Ok(())
    }
}
