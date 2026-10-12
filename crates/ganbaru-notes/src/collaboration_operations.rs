use crate::validation::require_uuid;
use serde_json::Value;
use sqlx::{Sqlite, Transaction};

pub struct CollaborationOperation<'a> {
    pub entity_type: &'static str,
    pub entity_id: &'a str,
    pub operation_type: &'static str,
    pub page_id: &'a str,
    pub block_id: Option<&'a str>,
    pub actor_id: &'a str,
    pub actor_display_name: &'a str,
    pub base_version: i64,
    pub entity_version: i64,
    pub conflict_policy: &'static str,
    pub payload: Value,
}

pub async fn record_tx(
    tx: &mut Transaction<'_, Sqlite>,
    operation: CollaborationOperation<'_>,
) -> Result<(), String> {
    require_uuid(operation.entity_id, "operation.entity_id")?;
    require_uuid(operation.page_id, "operation.page_id")?;
    if let Some(block_id) = operation.block_id {
        require_uuid(block_id, "operation.block_id")?;
    }
    require_uuid(operation.actor_id, "operation.actor_id")?;
    if operation.base_version < 0 || operation.entity_version <= operation.base_version {
        return Err("notes collaboration operation version is invalid".to_string());
    }
    crate::project_history::mark_page_dirty_tx(
        tx,
        operation.page_id,
        operation.operation_type,
        false,
    )
    .await?;
    let operation_id = new_operation_id_tx(tx).await?;
    sqlx::query(
        "INSERT INTO notes_collaboration_operations (
            id,
            entity_type,
            entity_id,
            operation_type,
            page_id,
            block_id,
            actor_id,
            actor_display_name,
            base_version,
            entity_version,
            conflict_policy,
            payload
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(operation_id)
    .bind(operation.entity_type)
    .bind(operation.entity_id)
    .bind(operation.operation_type)
    .bind(operation.page_id)
    .bind(operation.block_id)
    .bind(operation.actor_id)
    .bind(operation.actor_display_name)
    .bind(operation.base_version)
    .bind(operation.entity_version)
    .bind(operation.conflict_policy)
    .bind(operation.payload.to_string())
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("record notes collaboration operation: {e}"))?;
    Ok(())
}

async fn new_operation_id_tx(tx: &mut Transaction<'_, Sqlite>) -> Result<String, String> {
    for _ in 0..32 {
        let id: String = sqlx::query_scalar(
            "SELECT lower(hex(randomblob(4))) || '-' ||
                    lower(hex(randomblob(2))) || '-' ||
                    lower(hex(randomblob(2))) || '-' ||
                    lower(hex(randomblob(2))) || '-' ||
                    lower(hex(randomblob(6)))",
        )
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("generate notes collaboration operation id: {e}"))?;
        require_uuid(&id, "generated_operation_id")?;
        let exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1
             FROM notes_collaboration_operations
             WHERE id = ?",
        )
        .bind(&id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("check notes collaboration operation id: {e}"))?;
        if exists.is_none() {
            return Ok(id);
        }
    }
    Err("could not generate a unique notes collaboration operation id".to_string())
}
