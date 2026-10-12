use super::provider_not_found_error;
use ganbaru_chat_contracts::config::{ChatVaultConfig, RememberedComposerSelection};
use ganbaru_chat_contracts::models::{ChatResult, ProjectWorkingFolderId, ProviderInstanceId};

pub(crate) fn set_working_folder_provider_preference(
    config: &mut ChatVaultConfig,
    working_folder_id: ProjectWorkingFolderId,
    instance_id: Option<ProviderInstanceId>,
) -> ChatResult<()> {
    match instance_id {
        Some(instance_id) => {
            if !config
                .providers
                .iter()
                .any(|provider| provider.instance_id == instance_id)
            {
                return Err(provider_not_found_error());
            }
            config
                .working_folder_provider_preferences
                .insert(working_folder_id, instance_id);
        }
        None => {
            config
                .working_folder_provider_preferences
                .remove(&working_folder_id);
        }
    }
    Ok(())
}

pub(crate) fn remember_composer_selection(
    config: &mut ChatVaultConfig,
    selection: RememberedComposerSelection,
) -> ChatResult<()> {
    if !config
        .providers
        .iter()
        .any(|provider| provider.instance_id == selection.provider_instance_id)
    {
        return Err(provider_not_found_error());
    }
    config.remembered_selections.retain(|existing| {
        existing.working_folder_id != selection.working_folder_id
            || existing.provider_instance_id != selection.provider_instance_id
    });
    config.remembered_selections.push(selection);
    Ok(())
}
