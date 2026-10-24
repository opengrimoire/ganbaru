use crate::models::{
    NoteBlockUpdate, NoteBlockWrite, NoteDatabaseCreate, NoteFolderCreate, NoteFolderUpdate,
    NotePageCreate, NotePageUpdate, NoteParent,
};
use reqwest::Url;
use serde_json::Value;

mod assets;
mod block;
mod catalog;
mod media;
mod plain_text;
mod primitives;
mod requests;
mod rich_text;
mod web_reference;

use assets::*;
use block::*;
use catalog::*;
use media::*;
use rich_text::*;

pub use assets::{validate_icon_value, validate_native_icon, validate_page_cover_value};
pub use block::{MAX_TABLE_WIDTH, validate_block_payload};
#[allow(unused_imports)]
pub use catalog::{
    NOTE_BLOCK_TYPES, block_payload_supports_children, block_type_supports_children,
    validate_block_type,
};
pub use plain_text::{plain_text_from_payload, rich_text_items_plain_text};
pub use primitives::{
    validate_children_count, validate_duplicate_block_count, validate_page_size,
    validate_sort_order,
};
pub use requests::{
    require_uuid, validate_block_update, validate_block_write, validate_database_create,
    validate_folder_create, validate_folder_project_id, validate_folder_update,
    validate_page_create, validate_page_properties, validate_page_update, validate_parent,
};
pub use rich_text::{
    date_mention_reminder_minutes_before, validate_comment_body_rich_text,
    validate_comment_rich_text, validate_date_mention_reminder, validate_rich_text_url,
};
pub use web_reference::{
    is_exportable_web_reference_url, is_storable_bookmark_url, is_storable_web_content_url,
    uri_scheme,
};
