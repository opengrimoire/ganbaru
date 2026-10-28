//! Notes assets across replicas: a file block and its asset row replicate and receivers record
//! the reference and report the file for transfer, an asset row that arrives after the block
//! naming it refreshes that block, a remote block edit moves the reference, a remote template
//! edit changes the referenced files, a history pin keeps a file after its last block reference
//! is removed remotely, two replicas adding the same bytes agree on one row, also under two
//! content types, and a crash while sealing or applying asset rows leaves nothing partial.

use super::block_on;
use super::chat::after_rollback;
use super::notes::{BLOCK_A, BLOCK_B, CREATED, EDITED, assert_converged, shared, texts};
use super::projects::{PROJECT, project_sql};
use super::replica::{Replica, converge, quoted};
use crate::assets::AssetRef;

const FILES: &str = "notes/files";

fn digest(character: char) -> String {
    character.to_string().repeat(64)
}

fn asset_id(character: char) -> String {
    format!("{FILES}:{}", digest(character))
}

fn asset_path(character: char) -> String {
    format!("{FILES}/{}.pdf", digest(character))
}

/// SQL adding the PDF asset row of digest `character` with `size` bytes, named `name`.
fn asset_sql(character: char, size: u32, name: &str) -> String {
    format!(
        "INSERT INTO notes_assets
             (id, asset_path, kind, source_type, original_name, content_type, byte_size, sha256,
              created_at, updated_at)
         VALUES ({id}, {path}, 'pdf', 'local_upload', {name}, 'application/pdf', {size},
                 {digest}, {created}, {created});",
        id = quoted(&asset_id(character)),
        path = quoted(&asset_path(character)),
        name = quoted(name),
        digest = quoted(&digest(character)),
        created = quoted(CREATED),
    )
}

/// The file payload of a block naming the PDF of digest `character`.
fn file_payload(character: char, size: u32) -> String {
    let path = asset_path(character);
    serde_json::json!({
        "caption": [],
        "type": "file",
        "file": {
            "url": format!("ganbaru-asset:{path}"),
            "content_type": "application/pdf",
            "byte_size": size,
            "sha256": digest(character),
            "ganbaru_asset_path": path,
        }
    })
    .to_string()
}

/// SQL turning block A into a file block naming the PDF of digest `character`, with the plain
/// text Notes derives from it.
fn file_block_sql(character: char, size: u32) -> String {
    format!(
        "UPDATE notes_blocks
         SET type = 'file', attrs = {attrs}, plain_text = {url}, last_edited_time = {edited}
         WHERE id = {a};",
        attrs = quoted(&file_payload(character, size)),
        url = quoted(&format!("ganbaru-asset:{}", asset_path(character))),
        edited = quoted(EDITED),
        a = quoted(BLOCK_A),
    )
}

/// Block references as `asset_id role owner`.
async fn references(replica: &mut Replica) -> Vec<String> {
    texts(
        replica,
        "SELECT asset_id || ' ' || role || ' ' || owner_id FROM notes_asset_references
         ORDER BY asset_id, role, owner_id",
    )
    .await
}

/// The files the engine reports for transfer, as `class/file size`.
async fn transferable(replica: &mut Replica) -> Vec<String> {
    let referenced: Vec<AssetRef> = replica
        .engine
        .referenced_assets(&mut replica.conn)
        .await
        .unwrap()
        .into_iter()
        .filter(|asset| asset.class().name.starts_with("notes/"))
        .collect();
    referenced
        .iter()
        .map(|asset| {
            format!(
                "{}/{} {}",
                asset.class().name,
                asset.file_name(),
                asset.byte_size()
            )
        })
        .collect()
}

fn block_reference(character: char) -> String {
    format!("{} block_file {BLOCK_A}", asset_id(character))
}

#[test]
fn a_file_block_and_its_asset_row_replicate_and_receivers_reference_the_file() {
    block_on(async {
        let setup = format!(
            "{}{}",
            asset_sql('a', 2048, "report.pdf"),
            file_block_sql('a', 2048)
        );
        let mut replicas = shared(2, &setup).await;
        let reader = &mut replicas[1];
        assert_eq!(
            texts(
                reader,
                "SELECT id || ' ' || asset_path || ' ' || kind || ' ' || original_name || ' '
                     || byte_size || ' ' || storage_state || ' ' || coalesce(missing_at, '-')
                 FROM notes_assets",
            )
            .await,
            [format!(
                "{} {} pdf report.pdf 2048 available -",
                asset_id('a'),
                asset_path('a')
            )]
        );
        assert_eq!(references(reader).await, [block_reference('a')]);
        assert_eq!(
            transferable(reader).await,
            [format!("{FILES}/{}.pdf 2048", digest('a'))]
        );
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn an_asset_row_arriving_after_the_block_naming_it_refreshes_that_block() {
    block_on(async {
        let mut replicas = shared(2, &file_block_sql('a', 2048)).await;
        // The reader holds the block but not the row, so the block records no reference yet,
        // while the engine already reports the file for transfer.
        assert!(references(&mut replicas[1]).await.is_empty());
        assert_eq!(
            transferable(&mut replicas[1]).await,
            [format!("{FILES}/{}.pdf 2048", digest('a'))]
        );
        replicas[0].exec(&asset_sql('a', 2048, "report.pdf")).await;
        converge(&mut replicas).await;
        assert_eq!(references(&mut replicas[1]).await, [block_reference('a')]);
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn a_remote_block_edit_moves_the_reference_and_keeps_both_rows() {
    block_on(async {
        let setup = format!(
            "{}{}{}",
            asset_sql('a', 2048, "first.pdf"),
            asset_sql('b', 4096, "second.pdf"),
            file_block_sql('a', 2048)
        );
        let mut replicas = shared(2, &setup).await;
        assert_eq!(references(&mut replicas[1]).await, [block_reference('a')]);
        replicas[0].exec(&file_block_sql('b', 4096)).await;
        converge(&mut replicas).await;
        let reader = &mut replicas[1];
        assert_eq!(references(reader).await, [block_reference('b')]);
        assert_eq!(reader.count("SELECT count(*) FROM notes_assets").await, 2);
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn a_remote_template_edit_changes_the_files_the_receiver_transfers() {
    block_on(async {
        let template_block = |payload: &str| {
            format!(
                "BEGIN; DELETE FROM notes_page_template_blocks WHERE template_id = 'tpl';
                 INSERT INTO notes_page_template_blocks
                     (template_id, ordinal, id, parent_type, parent_block_id, type, payload,
                      plain_text, created_time, last_edited_time)
                 VALUES ('tpl', 0, 'tpl-file', 'template', NULL, 'file', {payload}, '',
                         {created}, {created});
                 COMMIT;",
                payload = quoted(payload),
                created = quoted(CREATED),
            )
        };
        let setup = format!(
            "{}{}INSERT INTO notes_page_templates (id, name, created_time, last_edited_time)
             VALUES ('tpl', 'Weekly', {created}, {created});
             {}",
            asset_sql('a', 2048, "first.pdf"),
            asset_sql('b', 4096, "second.pdf"),
            template_block(&file_payload('a', 2048)),
            created = quoted(CREATED),
        );
        let mut replicas = shared(2, &setup).await;
        assert_eq!(
            transferable(&mut replicas[1]).await,
            [format!("{FILES}/{}.pdf 2048", digest('a'))]
        );
        replicas[0]
            .exec(&template_block(&file_payload('b', 4096)))
            .await;
        converge(&mut replicas).await;
        // The replaced snapshot stays referenced while its version is retained.
        let files = transferable(&mut replicas[1]).await;
        assert!(
            files.contains(&format!("{FILES}/{}.pdf 4096", digest('b'))),
            "{files:?}"
        );
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn a_history_pin_keeps_the_file_after_its_last_block_reference_is_removed_remotely() {
    block_on(async {
        let setup = format!(
            "{}{}{}",
            project_sql(),
            asset_sql('a', 2048, "report.pdf"),
            file_block_sql('a', 2048)
        );
        let mut replicas = shared(2, &setup).await;
        // A project history version on the reader pins the asset. Versions and pins are device
        // rows, so the reader keeps them to itself.
        replicas[1]
            .exec(&format!(
                "INSERT INTO notes_history_bundles
                     (hash, kind, encoding, payload, uncompressed_bytes, stored_bytes)
                 VALUES ({manifest}, 'manifest', 'raw-json-v1', X'7B7D', 2, 2);
                 INSERT INTO notes_project_history_versions
                     (id, project_id, manifest_hash, reason, created_by, display_name)
                 VALUES ('version-1', {project}, {manifest}, 'manual', 'owner', '\"Before\"');
                 INSERT INTO notes_project_history_asset_pins (version_id, asset_id)
                 VALUES ('version-1', {id});",
                manifest = quoted(&digest('f')),
                project = quoted(PROJECT),
                id = quoted(&asset_id('a')),
            ))
            .await;
        replicas[0]
            .exec(&format!(
                "DELETE FROM notes_blocks WHERE id = {};",
                quoted(BLOCK_A)
            ))
            .await;
        converge(&mut replicas).await;
        let reader = &mut replicas[1];
        assert!(references(reader).await.is_empty());
        assert_eq!(reader.count("SELECT count(*) FROM notes_assets").await, 1);
        let kept = reader
            .engine
            .kept_asset_digests(&mut reader.conn)
            .await
            .unwrap();
        assert!(kept.contains(&(FILES, digest('a'))), "{kept:?}");
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn two_replicas_adding_the_same_bytes_agree_on_one_row() {
    block_on(async {
        let mut replicas = shared(2, "").await;
        replicas[0].exec(&asset_sql('a', 2048, "mine.pdf")).await;
        replicas[1].exec(&asset_sql('a', 2048, "theirs.pdf")).await;
        converge(&mut replicas).await;
        for replica in replicas.iter_mut() {
            assert_eq!(
                texts(
                    replica,
                    "SELECT id || ' ' || asset_path || ' ' || byte_size FROM notes_assets",
                )
                .await,
                [format!("{} {} 2048", asset_id('a'), asset_path('a'))]
            );
        }
        assert_converged(&mut replicas).await;
    });
}

/// SQL adding the asset row of digest `character` as `content_type` under `extension`, and
/// turning `block` into a file block naming that path.
fn typed_file_sql(block: &str, character: char, extension: &str, content_type: &str) -> String {
    let path = format!("{FILES}/{}.{extension}", digest(character));
    let payload = serde_json::json!({
        "caption": [],
        "type": "file",
        "file": {
            "url": format!("ganbaru-asset:{path}"),
            "content_type": content_type,
            "byte_size": 64,
            "sha256": digest(character),
            "ganbaru_asset_path": path,
        }
    })
    .to_string();
    format!(
        "INSERT INTO notes_assets
             (id, asset_path, kind, source_type, content_type, byte_size, sha256, created_at,
              updated_at)
         VALUES ({id}, {path}, 'file', 'local_upload', {content_type}, 64, {digest}, {created},
                 {created});
         UPDATE notes_blocks
         SET type = 'file', attrs = {attrs}, plain_text = {url}, last_edited_time = {edited}
         WHERE id = {block};",
        id = quoted(&asset_id(character)),
        path = quoted(&path),
        content_type = quoted(content_type),
        digest = quoted(&digest(character)),
        created = quoted(CREATED),
        attrs = quoted(&payload),
        url = quoted(&format!("ganbaru-asset:{path}")),
        edited = quoted(EDITED),
        block = quoted(block),
    )
}

#[test]
fn the_same_bytes_added_under_two_content_types_resolve_to_one_row_on_every_replica() {
    block_on(async {
        let mut replicas = shared(2, "").await;
        replicas[0]
            .exec(&typed_file_sql(BLOCK_A, 'c', "txt", "text/plain"))
            .await;
        replicas[1]
            .exec(&typed_file_sql(BLOCK_B, 'c', "csv", "text/csv"))
            .await;
        converge(&mut replicas).await;
        // Raw SQL records no local references, so each replica checks the block it received.
        // The replica whose row lost the content merge also rescans its own block by digest.
        for (replica, received) in replicas.iter_mut().zip([BLOCK_B, BLOCK_A]) {
            assert_eq!(
                replica.count("SELECT count(*) FROM notes_assets").await,
                1,
                "one id names the bytes"
            );
            assert_eq!(
                texts(
                    replica,
                    "SELECT id || ' ' || json_extract(attrs, '$.file.ganbaru_asset_path')
                     FROM notes_blocks WHERE type = 'file' ORDER BY id",
                )
                .await,
                [
                    format!("{BLOCK_A} {FILES}/{}.txt", digest('c')),
                    format!("{BLOCK_B} {FILES}/{}.csv", digest('c')),
                ],
                "each block keeps the path it named"
            );
            let references = references(replica).await;
            assert!(
                references.contains(&format!("{} block_file {received}", asset_id('c'))),
                "the received block references the one row whatever its path: {references:?}"
            );
        }
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn a_crash_while_sealing_or_applying_asset_rows_leaves_nothing_partial() {
    block_on(async {
        let mut replicas = shared(2, "").await;
        replicas[0]
            .exec(&format!(
                "{}{}",
                asset_sql('a', 2048, "report.pdf"),
                file_block_sql('a', 2048)
            ))
            .await;
        for name in ["seal.reserve", "seal.before_commit"] {
            let report = after_rollback(&mut replicas[0], name, async |r: &mut Replica| {
                r.seal().await
            })
            .await;
            assert_eq!(report.sealed_ops, 1, "{name}: the rows seal together");
            if name == "seal.reserve" {
                // A second file and the block moving to it give the next failpoint rows to seal.
                replicas[0]
                    .exec(&format!(
                        "{}{}",
                        asset_sql('b', 4096, "second.pdf"),
                        file_block_sql('b', 4096)
                    ))
                    .await;
            }
        }

        let (author, rest) = replicas.split_at_mut(1);
        let reader = &mut rest[0];
        let known = reader.stored_vector().await;
        let ops = author[0].ops_since(&known).await;
        reader.receive(&ops).await;
        after_rollback(reader, "apply.before_commit", async |r: &mut Replica| {
            r.apply_all().await
        })
        .await;
        assert_eq!(reader.count("SELECT count(*) FROM notes_assets").await, 2);
        assert_eq!(references(reader).await, [block_reference('b')]);
        assert_converged(&mut replicas).await;
    });
}
