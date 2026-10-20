use ganbaru_notes::transfers::notion_api_import::{NotionImportCancellation, validate_import_id};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Most Notion API imports that can be addressed for stopping at once.
const MAX_ACTIVE_IMPORTS: usize = 16;

/// Stop signals for running Notion API imports, keyed by the client-chosen import ID.
#[derive(Default)]
pub struct NotionApiImportCancellations {
    active: Mutex<HashMap<String, NotionImportCancellation>>,
}

impl NotionApiImportCancellations {
    /// Registers a running import; the returned guard removes it when the import ends.
    pub(crate) fn register(&self, import_id: &str) -> Result<RegisteredNotionImport<'_>, String> {
        let import_id = validate_import_id(import_id)?.to_string();
        let mut active = self.lock();
        if active.contains_key(&import_id) {
            return Err("a Notion import with this ID is already running".to_string());
        }
        if active.len() >= MAX_ACTIVE_IMPORTS {
            return Err("too many Notion imports are running".to_string());
        }
        let cancellation = NotionImportCancellation::new();
        active.insert(import_id.clone(), cancellation.clone());
        Ok(RegisteredNotionImport {
            registry: self,
            import_id,
            cancellation,
        })
    }

    /// Signals the import to stop; `false` when no import with this ID is running.
    pub(crate) fn cancel(&self, import_id: &str) -> Result<bool, String> {
        let import_id = validate_import_id(import_id)?;
        let Some(cancellation) = self.lock().get(import_id).cloned() else {
            return Ok(false);
        };
        cancellation.cancel();
        Ok(true)
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, NotionImportCancellation>> {
        self.active.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Registration of one running import, removed on every exit path when dropped.
pub(crate) struct RegisteredNotionImport<'a> {
    registry: &'a NotionApiImportCancellations,
    import_id: String,
    cancellation: NotionImportCancellation,
}

impl RegisteredNotionImport<'_> {
    pub(crate) fn cancellation(&self) -> &NotionImportCancellation {
        &self.cancellation
    }
}

impl Drop for RegisteredNotionImport<'_> {
    fn drop(&mut self) {
        self.registry.lock().remove(&self.import_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_reaches_the_registered_import_until_it_ends() {
        let imports = NotionApiImportCancellations::default();
        let registered = imports.register("import-1").unwrap();

        assert!(imports.cancel("import-1").unwrap());
        assert!(registered.cancellation().is_canceled());

        drop(registered);
        assert!(!imports.cancel("import-1").unwrap());
        assert!(imports.register("import-1").is_ok());
    }

    #[test]
    fn registration_rejects_duplicate_invalid_and_excess_ids() {
        let imports = NotionApiImportCancellations::default();
        let _first = imports.register("import-1").unwrap();

        assert!(imports.register("import-1").is_err());
        assert!(imports.register("bad id").is_err());
        assert!(imports.cancel("bad/id").is_err());
        let held = (2..=MAX_ACTIVE_IMPORTS)
            .map(|index| imports.register(&format!("import-{index}")).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(held.len(), MAX_ACTIVE_IMPORTS - 1);
        assert!(imports.register("import-overflow").is_err());
    }
}
