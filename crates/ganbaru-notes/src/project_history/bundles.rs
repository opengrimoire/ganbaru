use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Sqlite, Transaction};
use std::io::{Read, Write};

const HISTORY_BUNDLE_CHUNK_BYTES: usize = 8 * 1024 * 1024;
const MAX_HISTORY_BUNDLE_BYTES: usize = 256 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
struct ChunkedBundleDescriptor {
    format: String,
    uncompressed_bytes: usize,
    chunk_hashes: Vec<String>,
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub(super) async fn store_bundle_tx(
    tx: &mut Transaction<'_, Sqlite>,
    kind: &str,
    raw: &[u8],
) -> Result<String, String> {
    if raw.len() > MAX_HISTORY_BUNDLE_BYTES {
        return Err("Notes history bundle exceeds the 256 MB safety limit".to_string());
    }
    let hash = sha256_hex(raw);
    if raw.len() <= HISTORY_BUNDLE_CHUNK_BYTES {
        store_single_bundle_tx(tx, &hash, kind, raw, true).await?;
        return Ok(hash);
    }
    let mut chunk_hashes = Vec::new();
    for chunk in raw.chunks(HISTORY_BUNDLE_CHUNK_BYTES) {
        let chunk_hash = sha256_hex(chunk);
        store_single_bundle_tx(tx, &chunk_hash, "chunk", chunk, false).await?;
        chunk_hashes.push(chunk_hash);
    }
    let descriptor = ChunkedBundleDescriptor {
        format: "chunked-json-v1".to_string(),
        uncompressed_bytes: raw.len(),
        chunk_hashes: chunk_hashes.clone(),
    };
    let payload = serde_json::to_vec(&descriptor)
        .map_err(|e| format!("serialize Notes history chunk descriptor: {e}"))?;
    sqlx::query(
        "INSERT OR IGNORE INTO notes_history_bundles (
            hash, kind, encoding, payload, uncompressed_bytes, stored_bytes
         ) VALUES (?, ?, 'chunked-json-v1', ?, ?, ?)",
    )
    .bind(&hash)
    .bind(kind)
    .bind(&payload)
    .bind(i64::try_from(raw.len()).map_err(|_| "Notes history bundle is too large".to_string())?)
    .bind(
        i64::try_from(payload.len())
            .map_err(|_| "Notes history bundle descriptor is too large".to_string())?,
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("store chunked Notes history bundle: {e}"))?;
    for (chunk_index, chunk_hash) in chunk_hashes.iter().enumerate() {
        sqlx::query(
            "INSERT OR IGNORE INTO notes_history_bundle_chunks (
                parent_hash, chunk_index, chunk_hash
             ) VALUES (?, ?, ?)",
        )
        .bind(&hash)
        .bind(
            i64::try_from(chunk_index)
                .map_err(|_| "Notes history bundle has too many chunks".to_string())?,
        )
        .bind(chunk_hash)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("store Notes history bundle chunk reference: {e}"))?;
    }
    Ok(hash)
}

async fn store_single_bundle_tx(
    tx: &mut Transaction<'_, Sqlite>,
    hash: &str,
    kind: &str,
    raw: &[u8],
    is_json: bool,
) -> Result<(), String> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(raw)
        .map_err(|e| format!("compress Notes history bundle: {e}"))?;
    let compressed = encoder
        .finish()
        .map_err(|e| format!("finish Notes history bundle compression: {e}"))?;
    let (encoding, payload) = if compressed.len() < raw.len() {
        (
            if is_json {
                "zlib-json-v1"
            } else {
                "zlib-bytes-v1"
            },
            compressed,
        )
    } else {
        (
            if is_json {
                "raw-json-v1"
            } else {
                "raw-bytes-v1"
            },
            raw.to_vec(),
        )
    };
    sqlx::query(
        "INSERT OR IGNORE INTO notes_history_bundles (
            hash, kind, encoding, payload, uncompressed_bytes, stored_bytes
         ) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(hash)
    .bind(kind)
    .bind(encoding)
    .bind(&payload)
    .bind(i64::try_from(raw.len()).map_err(|_| "Notes history bundle is too large".to_string())?)
    .bind(
        i64::try_from(payload.len())
            .map_err(|_| "Notes history bundle is too large".to_string())?,
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("store Notes history bundle: {e}"))?;
    Ok(())
}

pub(super) async fn load_bundle_tx(
    tx: &mut Transaction<'_, Sqlite>,
    hash: &str,
) -> Result<Vec<u8>, String> {
    let row: Option<(String, Vec<u8>, i64)> = sqlx::query_as(
        "SELECT encoding, payload, uncompressed_bytes
         FROM notes_history_bundles
         WHERE hash = ?",
    )
    .bind(hash)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load Notes history bundle: {e}"))?;
    let (encoding, payload, uncompressed_bytes) =
        row.ok_or_else(|| "Notes history bundle not found".to_string())?;
    let expected_size = usize::try_from(uncompressed_bytes)
        .map_err(|_| "Notes history bundle size is invalid".to_string())?;
    if expected_size > MAX_HISTORY_BUNDLE_BYTES {
        return Err("Notes history bundle exceeds the 256 MB safety limit".to_string());
    }
    let raw = match encoding.as_str() {
        "raw-json-v1" | "raw-bytes-v1" => payload,
        "zlib-json-v1" | "zlib-bytes-v1" => {
            let decoder = ZlibDecoder::new(payload.as_slice());
            let mut limited = decoder.take((expected_size as u64).saturating_add(1));
            let mut decoded = Vec::with_capacity(expected_size);
            limited
                .read_to_end(&mut decoded)
                .map_err(|e| format!("decompress Notes history bundle: {e}"))?;
            decoded
        }
        "chunked-json-v1" => {
            let descriptor: ChunkedBundleDescriptor = serde_json::from_slice(&payload)
                .map_err(|e| format!("parse Notes history chunk descriptor: {e}"))?;
            if descriptor.format != "chunked-json-v1"
                || descriptor.uncompressed_bytes != expected_size
            {
                return Err("Notes history chunk descriptor is invalid".to_string());
            }
            let stored_chunks: Vec<(i64, String)> = sqlx::query_as(
                "SELECT chunk_index, chunk_hash
                 FROM notes_history_bundle_chunks
                 WHERE parent_hash = ?
                 ORDER BY chunk_index",
            )
            .bind(hash)
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| format!("load Notes history bundle chunks: {e}"))?;
            if stored_chunks.len() != descriptor.chunk_hashes.len()
                || stored_chunks
                    .iter()
                    .enumerate()
                    .any(|(index, (stored_index, stored_hash))| {
                        usize::try_from(*stored_index).ok() != Some(index)
                            || descriptor.chunk_hashes.get(index) != Some(stored_hash)
                    })
            {
                return Err("Notes history bundle chunk references are invalid".to_string());
            }
            let mut decoded = Vec::with_capacity(expected_size);
            for (_, chunk_hash) in stored_chunks {
                let chunk = load_chunk_tx(tx, &chunk_hash).await?;
                if decoded.len().saturating_add(chunk.len()) > expected_size {
                    return Err("Notes history chunk data exceeds its declared size".to_string());
                }
                decoded.extend_from_slice(&chunk);
            }
            decoded
        }
        _ => return Err("Notes history bundle encoding is unsupported".to_string()),
    };
    if raw.len() != expected_size {
        return Err("Notes history bundle size does not match its metadata".to_string());
    }
    if sha256_hex(&raw) != hash {
        return Err("Notes history bundle failed its SHA-256 integrity check".to_string());
    }
    Ok(raw)
}

async fn load_chunk_tx(tx: &mut Transaction<'_, Sqlite>, hash: &str) -> Result<Vec<u8>, String> {
    let row: Option<(String, Vec<u8>, i64)> = sqlx::query_as(
        "SELECT encoding, payload, uncompressed_bytes
         FROM notes_history_bundles
         WHERE hash = ?",
    )
    .bind(hash)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load Notes history chunk: {e}"))?;
    let (encoding, payload, uncompressed_bytes) =
        row.ok_or_else(|| "Notes history chunk not found".to_string())?;
    let expected_size = usize::try_from(uncompressed_bytes)
        .map_err(|_| "Notes history chunk size is invalid".to_string())?;
    if expected_size > HISTORY_BUNDLE_CHUNK_BYTES {
        return Err("Notes history chunk exceeds the 8 MB safety limit".to_string());
    }
    let raw = match encoding.as_str() {
        "raw-bytes-v1" | "raw-json-v1" => payload,
        "zlib-bytes-v1" | "zlib-json-v1" => {
            let decoder = ZlibDecoder::new(payload.as_slice());
            let mut limited = decoder.take((expected_size as u64).saturating_add(1));
            let mut decoded = Vec::with_capacity(expected_size);
            limited
                .read_to_end(&mut decoded)
                .map_err(|e| format!("decompress Notes history chunk: {e}"))?;
            decoded
        }
        _ => return Err("Notes history chunk encoding is unsupported".to_string()),
    };
    if raw.len() != expected_size || sha256_hex(&raw) != hash {
        return Err("Notes history chunk failed its integrity check".to_string());
    }
    Ok(raw)
}

pub(super) async fn garbage_collect_bundles_tx(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<(), String> {
    sqlx::query(
        "DELETE FROM notes_history_bundle_chunks
         WHERE parent_hash NOT IN (
             SELECT manifest_hash FROM notes_project_history_versions
             UNION
             SELECT bundle_hash FROM notes_project_history_bundle_references
             UNION
             SELECT block_bundle_hash
             FROM notes_page_history_snapshots
             WHERE block_bundle_hash IS NOT NULL
         )",
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("garbage collect Notes history chunk references: {e}"))?;
    sqlx::query(
        "DELETE FROM notes_history_bundles
         WHERE NOT EXISTS (
             SELECT 1
             FROM notes_project_history_versions
             WHERE manifest_hash = notes_history_bundles.hash
         )
           AND NOT EXISTS (
             SELECT 1
             FROM notes_project_history_bundle_references
             WHERE bundle_hash = notes_history_bundles.hash
         )
           AND NOT EXISTS (
             SELECT 1
             FROM notes_history_bundle_chunks
             WHERE chunk_hash = notes_history_bundles.hash
         )
           AND NOT EXISTS (
             SELECT 1
             FROM notes_page_history_snapshots
             WHERE block_bundle_hash = notes_history_bundles.hash
         )",
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("garbage collect Notes history bundles: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MAX_HISTORY_BUNDLE_BYTES, load_bundle_tx, sha256_hex, store_bundle_tx};
    use ganbaru_db::run_migrations;

    async fn migrated_pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql("PRAGMA foreign_keys=ON")
            .execute(&pool)
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    #[test]
    fn sha256_is_stable_for_canonical_payloads() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn bundles_round_trip_deduplicate_and_reject_tampering() {
        crate::test_block_on(async {
            let pool = migrated_pool().await;
            let payload = br#"{"project":"Learning","pages":[1,2,3]}"#;
            let mut tx = pool.begin().await.unwrap();
            let first = store_bundle_tx(&mut tx, "row", payload).await.unwrap();
            let second = store_bundle_tx(&mut tx, "row", payload).await.unwrap();
            assert_eq!(first, second);
            assert_eq!(load_bundle_tx(&mut tx, &first).await.unwrap(), payload);
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_history_bundles")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            assert_eq!(count, 1);
            sqlx::query(
                "UPDATE notes_history_bundles
                 SET payload = x'00', stored_bytes = 1
                 WHERE hash = ?",
            )
            .bind(&first)
            .execute(&mut *tx)
            .await
            .unwrap();
            assert!(load_bundle_tx(&mut tx, &first).await.is_err());
        });
    }

    #[test]
    fn identical_bytes_deduplicate_across_bundle_roles() {
        crate::test_block_on(async {
            let pool = migrated_pool().await;
            let payload = br#"{"schema_version":1}"#;
            let mut tx = pool.begin().await.unwrap();
            let row_hash = store_bundle_tx(&mut tx, "row", payload).await.unwrap();
            let manifest_hash = store_bundle_tx(&mut tx, "manifest", payload).await.unwrap();

            assert_eq!(row_hash, manifest_hash);
            assert_eq!(load_bundle_tx(&mut tx, &row_hash).await.unwrap(), payload);
            let stored: (i64, String) = sqlx::query_as(
                "SELECT COUNT(*), MIN(kind) FROM notes_history_bundles WHERE hash = ?",
            )
            .bind(&row_hash)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
            assert_eq!(stored, (1, "row".to_string()));
        });
    }

    #[test]
    fn chunked_bundles_round_trip_with_bounded_decompression() {
        crate::test_block_on(async {
            let pool = migrated_pool().await;
            let payload = vec![b'a'; 8 * 1024 * 1024 + 17];
            let mut tx = pool.begin().await.unwrap();
            let hash = store_bundle_tx(&mut tx, "row", &payload).await.unwrap();
            assert_eq!(load_bundle_tx(&mut tx, &hash).await.unwrap(), payload);
            let chunks: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM notes_history_bundle_chunks WHERE parent_hash = ?",
            )
            .bind(&hash)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
            assert_eq!(chunks, 2);
            sqlx::query("UPDATE notes_history_bundles SET uncompressed_bytes = ? WHERE hash = ?")
                .bind(i64::try_from(MAX_HISTORY_BUNDLE_BYTES).unwrap() + 1)
                .bind(&hash)
                .execute(&mut *tx)
                .await
                .unwrap();
            assert!(load_bundle_tx(&mut tx, &hash).await.is_err());
        });
    }
}
