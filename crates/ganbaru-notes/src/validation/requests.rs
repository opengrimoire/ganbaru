use super::*;

pub fn require_uuid(value: &str, field: &str) -> Result<(), String> {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return Err(format!("{field} must be a UUID"));
    }
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 8 | 13 | 18 | 23) {
            if *byte != b'-' {
                return Err(format!("{field} must be a UUID"));
            }
            continue;
        }
        if !byte.is_ascii_hexdigit() {
            return Err(format!("{field} must be a UUID"));
        }
    }
    Ok(())
}

pub fn validate_page_create(page: &NotePageCreate) -> Result<(), String> {
    require_uuid(&page.id, "id")?;
    require_uuid(&page.first_block_id, "first_block_id")?;
    if page.id.trim() == page.first_block_id.trim() {
        return Err("first_block_id must not match id".to_string());
    }
    if let Some(after_block_id) = &page.after_block_id {
        require_uuid(after_block_id, "after_block_id")?;
    }
    if let Some(properties) = &page.properties {
        validate_json_object(properties, "properties")?;
    }
    if let Some(folder_id) = page.folder_id.as_deref() {
        require_uuid(folder_id, "folder_id")?;
        if !matches!(page.parent, NoteParent::Workspace { workspace: true }) {
            return Err("folder_id requires a workspace parent".to_string());
        }
    }
    validate_parent(&page.parent)
}

pub fn validate_folder_create(folder: &NoteFolderCreate) -> Result<(), String> {
    require_uuid(folder.id.trim(), "id")?;
    validate_folder_project_id(&folder.project_id)?;
    if let Some(parent_folder_id) = folder.parent_folder_id.as_deref() {
        require_uuid(parent_folder_id.trim(), "parent_folder_id")?;
    }
    validate_folder_name(&folder.name)
}

pub fn validate_folder_update(folder: &NoteFolderUpdate) -> Result<(), String> {
    if let Some(parent_folder_id) = folder.parent_folder_id.as_deref() {
        require_uuid(parent_folder_id.trim(), "parent_folder_id")?;
    }
    validate_folder_name(&folder.name)
}

pub fn validate_folder_project_id(project_id: &str) -> Result<(), String> {
    let project_id = project_id.trim();
    if project_id.is_empty() || project_id.len() > 120 {
        return Err("project_id is invalid".to_string());
    }
    Ok(())
}

fn validate_folder_name(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("folder name must not be empty".to_string());
    }
    if name.chars().count() > 200 {
        return Err("folder name must not exceed 200 characters".to_string());
    }
    if name.chars().any(char::is_control) {
        return Err("folder name must not contain control characters".to_string());
    }
    Ok(())
}

pub fn validate_page_update(update: &NotePageUpdate) -> Result<(), String> {
    if let Some(parent) = &update.parent {
        validate_parent(parent)?;
    }
    if let Some(properties) = &update.properties {
        validate_json_object(properties, "properties")?;
    }
    if let Some(icon) = update.icon.value() {
        validate_icon_value(icon, "icon")?;
    }
    if let Some(cover) = update.cover.value() {
        validate_page_cover_value(cover)?;
    }
    Ok(())
}

pub fn validate_database_create(request: &NoteDatabaseCreate) -> Result<(), String> {
    require_uuid(&request.id, "id")?;
    require_uuid(&request.data_source_id, "data_source_id")?;
    require_uuid(&request.view_id, "view_id")?;
    if request.id == request.data_source_id || request.id == request.view_id {
        return Err("database ids must be unique".to_string());
    }
    if request.data_source_id == request.view_id {
        return Err("data_source_id and view_id must be unique".to_string());
    }
    if contains_control_characters(&request.title) {
        return Err("title must not contain control characters".to_string());
    }
    match &request.replace_block_id {
        Some(block_id) => {
            require_uuid(block_id, "replace_block_id")?;
            if block_id.trim() != request.id.trim() {
                return Err("replace_block_id must match id".to_string());
            }
            if request.parent.is_some() {
                return Err("parent must not be set when replace_block_id is set".to_string());
            }
            if request.after_block_id.is_some() {
                return Err(
                    "after_block_id must not be set when replace_block_id is set".to_string(),
                );
            }
        }
        None => {
            let parent = request
                .parent
                .as_ref()
                .ok_or_else(|| "parent is required".to_string())?;
            validate_parent(parent)?;
            if matches!(parent, NoteParent::Workspace { .. }) {
                return Err("database blocks cannot be parented by workspace".to_string());
            }
            if matches!(parent, NoteParent::DataSourceId { .. }) {
                return Err("database blocks cannot be parented by data sources".to_string());
            }
            if let Some(after_block_id) = &request.after_block_id {
                require_uuid(after_block_id, "after_block_id")?;
            }
        }
    }
    if let Some(icon) = &request.icon {
        validate_icon_value(icon, "icon")?;
    }
    if let Some(cover) = &request.cover {
        validate_page_cover_value(cover)?;
    }
    Ok(())
}

pub fn validate_parent(parent: &NoteParent) -> Result<(), String> {
    match parent {
        NoteParent::Workspace { workspace } => {
            if *workspace {
                Ok(())
            } else {
                Err("workspace parent must set workspace to true".to_string())
            }
        }
        NoteParent::PageId { page_id } => require_uuid(page_id, "page_id"),
        NoteParent::BlockId { block_id } => require_uuid(block_id, "block_id"),
        NoteParent::DataSourceId { data_source_id } => {
            require_uuid(data_source_id, "data_source_id")
        }
    }
}

pub fn validate_block_write(block: &NoteBlockWrite) -> Result<(), String> {
    require_uuid(&block.id, "id")?;
    validate_block_type(&block.block_type)?;
    let payload = block
        .payload()
        .ok_or_else(|| format!("{} payload is required", block.block_type))?;
    validate_block_payload(&block.block_type, payload)
}

pub fn validate_block_update(
    current_type: &str,
    update: &NoteBlockUpdate,
) -> Result<(String, Value), String> {
    let block_type = update
        .block_type
        .as_deref()
        .unwrap_or(current_type)
        .trim()
        .to_string();
    validate_block_type(&block_type)?;
    let payload = update
        .payload_for(&block_type)
        .ok_or_else(|| format!("{block_type} payload is required"))?
        .clone();
    validate_block_payload(&block_type, &payload)?;
    Ok((block_type, payload))
}
