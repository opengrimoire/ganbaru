use super::{device_state_error, store::read_chat_config};
use crate::chat::device_state::update_active_device_scope;
use ganbaru_chat_contracts::models::{ChatResult, CredentialReferenceId};
use std::collections::BTreeSet;

pub(crate) fn invalidate_provider_state_for_credential(
    app: &tauri::AppHandle,
    reference_id: &CredentialReferenceId,
) -> ChatResult<()> {
    let affected = read_chat_config(app)?
        .providers
        .into_iter()
        .filter(|provider| {
            provider
                .credential_references
                .values()
                .any(|reference| reference == reference_id)
        })
        .map(|provider| provider.instance_id)
        .collect::<BTreeSet<_>>();
    if affected.is_empty() {
        return Ok(());
    }
    update_active_device_scope(app, |scope| {
        for instance_id in &affected {
            if let Some(provider) = scope.provider_instances.get_mut(instance_id) {
                provider.last_probe = None;
                if let Some(catalog) = provider.model_catalog.as_mut() {
                    catalog.stale = true;
                }
            }
        }
        Ok(())
    })
    .map_err(device_state_error)
}
