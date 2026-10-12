//! Composer interaction services behind the Tauri command facade.

mod attachments;
mod drafts;
mod followups;
mod runtime_operations;
mod state;
mod support;
mod workspace_mentions;

pub(crate) use attachments::{
    MAX_IMAGE_COUNT, attachment_data_url, import_image, import_text_snippet, pick_images,
    read_attachments,
};
pub(crate) use drafts::{read_user_input_draft, save_user_input_draft};
pub(crate) use followups::{
    cancel_queued_followup, mark_queued_followup_dispatched, save_queued_followup,
};
pub(crate) use runtime_operations::{
    compact_context, has_full_access_trust, read_mcp_status, set_full_access_trust, stop_session,
};
pub(crate) use state::read_interaction_state;
pub(crate) use workspace_mentions::{
    list_prompt_catalog, search_working_folder_paths, validate_working_folder_mentions,
    workspace_mention_is_safety_excluded,
};
