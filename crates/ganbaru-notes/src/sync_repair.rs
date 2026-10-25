//! Notes structure repair after sync apply.
//!
//! Merged placement can leave Notes structure the editor never writes: two devices each nest a
//! block under the other's block, a chain of nested blocks grows deeper than
//! [`MAX_BLOCK_DEPTH`](crate::writes::MAX_BLOCK_DEPTH), or two pages each become nested in the
//! other through their `child_page` blocks. Folders and row parents keep the engine tree repair;
//! blocks and nested pages have two-sided placement the engine does not walk, so Notes repairs
//! them here in one ordinary local transaction that sync captures and replicates.
//!
//! The rules are deterministic given the same rows:
//!
//! - A block in a cycle: the smallest block id of the cycle becomes a root block of its page,
//!   after that page's last root block. Its page is the hook-derived `page_id` of that block, or
//!   else the page of the smallest cycle member with a known page. A cycle without any known
//!   page stays unplaced and is counted in the report.
//! - A block deeper than the bound: the block at depth `MAX_BLOCK_DEPTH + 1` of its ancestry
//!   becomes a root block of the page that ancestry reaches.
//! - A page nested in itself through `child_page` blocks: the smallest page id of the cycle
//!   moves to the workspace, and its `child_page` block is deleted.
//!
//! The hook-derived `page_id` of a block in a cycle is the value it held before the cycle
//! formed, so devices that received the operations in a different order can derive different
//! pages for the same cycle. Repair runs only where Notes is writable, which keeps one device
//! deciding while Notes has a single writer.

use crate::writes::MAX_BLOCK_DEPTH;
use ganbaru_sync_contracts::OrderKey;
use sqlx::{Connection, SqliteConnection, SqlitePool};
use std::collections::{BTreeMap, BTreeSet};

/// What one repair pass changed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StructureRepair {
    /// Blocks moved to the root of a page to break a cycle.
    pub cycle_blocks: Vec<String>,
    /// Blocks moved to the root of a page because their ancestry was too deep.
    pub deep_blocks: Vec<String>,
    /// Smallest block id of each cycle no page could be found for.
    pub unplaced_blocks: Vec<String>,
    /// Pages moved to the workspace to break a nesting cycle.
    pub detached_pages: Vec<String>,
}

impl StructureRepair {
    /// Whether the pass changed nothing.
    pub fn is_empty(&self) -> bool {
        self.cycle_blocks.is_empty()
            && self.deep_blocks.is_empty()
            && self.detached_pages.is_empty()
    }
}

/// One block's placement as repair reads it.
#[derive(Debug, Clone, sqlx::FromRow)]
struct Placement {
    id: String,
    parent_page_id: Option<String>,
    parent_block_id: Option<String>,
    page_id: String,
}

/// Where one block's ancestry ends.
enum Walk {
    /// The ancestry reaches `page`; `chain` runs from the block up to its root block.
    Page { page: String, chain: Vec<String> },
    /// The ancestry loops; `members` are the blocks of the loop.
    Cycle { members: Vec<String> },
    /// The ancestry names a block that does not exist.
    Broken,
}

/// Repairs the structure around the changed `page_ids` and `block_ids` in one local transaction.
pub async fn repair_structure(
    pool: &SqlitePool,
    page_ids: &[String],
    block_ids: &[String],
) -> Result<StructureRepair, String> {
    let mut conn = pool
        .acquire()
        .await
        .map_err(|error| format!("acquire Notes structure repair connection: {error}"))?;
    repair_structure_on(&mut conn, page_ids, block_ids).await
}

/// [`repair_structure`] on one connection.
pub async fn repair_structure_on(
    conn: &mut SqliteConnection,
    page_ids: &[String],
    block_ids: &[String],
) -> Result<StructureRepair, String> {
    let mut tx = conn
        .begin()
        .await
        .map_err(|error| format!("begin Notes structure repair: {error}"))?;
    let mut report = StructureRepair::default();
    repair_blocks(&mut tx, block_ids, &mut report).await?;
    repair_page_nesting(&mut tx, page_ids, block_ids, &mut report).await?;
    tx.commit()
        .await
        .map_err(|error| format!("commit Notes structure repair: {error}"))?;
    Ok(report)
}

fn keys_json(keys: &[String]) -> Result<String, String> {
    serde_json::to_string(keys).map_err(|error| format!("encode Notes repair keys: {error}"))
}

/// Loads `block_ids`, their descendants within the depth bound, and every ancestor of those.
async fn load_placements(
    conn: &mut SqliteConnection,
    block_ids: &[String],
) -> Result<BTreeMap<String, Placement>, String> {
    let rows: Vec<Placement> = sqlx::query_as(
        "WITH RECURSIVE
         seed(id) AS (SELECT value FROM json_each(?1)),
         down(id, depth) AS (
             SELECT id, 0 FROM notes_blocks WHERE id IN seed
             UNION
             SELECT child.id, down.depth + 1
             FROM notes_blocks AS child
             JOIN down ON child.parent_block_id = down.id
             WHERE down.depth < ?2
         ),
         up(id) AS (
             SELECT id FROM down
             UNION
             SELECT block.parent_block_id
             FROM notes_blocks AS block
             JOIN up ON block.id = up.id
             WHERE block.parent_block_id IS NOT NULL
         )
         SELECT id, parent_page_id, parent_block_id, page_id
         FROM notes_blocks WHERE id IN up",
    )
    .bind(keys_json(block_ids)?)
    .bind(MAX_BLOCK_DEPTH)
    .fetch_all(&mut *conn)
    .await
    .map_err(|error| format!("load Notes block placement for repair: {error}"))?;
    Ok(rows.into_iter().map(|row| (row.id.clone(), row)).collect())
}

fn walk(placements: &BTreeMap<String, Placement>, start: &str) -> Walk {
    let mut chain: Vec<String> = Vec::new();
    let mut position: BTreeMap<String, usize> = BTreeMap::new();
    let mut current = start.to_string();
    loop {
        let Some(row) = placements.get(&current) else {
            return Walk::Broken;
        };
        position.insert(current.clone(), chain.len());
        chain.push(current.clone());
        if let Some(page) = &row.parent_page_id {
            return Walk::Page {
                page: page.clone(),
                chain,
            };
        }
        let Some(parent) = &row.parent_block_id else {
            return Walk::Broken;
        };
        if let Some(&at) = position.get(parent) {
            return Walk::Cycle {
                members: chain.split_off(at),
            };
        }
        current = parent.clone();
    }
}

/// Breaks block cycles and over-deep ancestry among the loaded blocks, in block id order.
async fn repair_blocks(
    conn: &mut SqliteConnection,
    block_ids: &[String],
    report: &mut StructureRepair,
) -> Result<(), String> {
    if block_ids.is_empty() {
        return Ok(());
    }
    let mut placements = load_placements(conn, block_ids).await?;
    let mut unplaced: BTreeSet<String> = BTreeSet::new();
    let depth_bound = usize::try_from(MAX_BLOCK_DEPTH)
        .map_err(|error| format!("convert Notes block depth bound: {error}"))?;
    let ids: Vec<String> = placements.keys().cloned().collect();
    let mut moved: Vec<String> = Vec::new();
    for id in ids {
        loop {
            match walk(&placements, &id) {
                Walk::Broken => break,
                Walk::Page { page, chain } => {
                    if chain.len() <= depth_bound {
                        break;
                    }
                    let target = chain[chain.len() - depth_bound - 1].clone();
                    move_to_page_root(conn, &mut placements, &target, &page).await?;
                    report.deep_blocks.push(target.clone());
                    moved.push(target);
                }
                Walk::Cycle { members } => {
                    let Some(chosen) = members.iter().min().cloned() else {
                        break;
                    };
                    if unplaced.contains(&chosen) {
                        break;
                    }
                    match cycle_page(conn, &placements, &chosen, &members).await? {
                        Some(page) => {
                            move_to_page_root(conn, &mut placements, &chosen, &page).await?;
                            report.cycle_blocks.push(chosen.clone());
                            moved.push(chosen);
                        }
                        None => {
                            unplaced.insert(chosen.clone());
                            report.unplaced_blocks.push(chosen);
                            break;
                        }
                    }
                }
            }
        }
    }
    if !moved.is_empty() {
        crate::sync_apply::refresh_block_pages(conn, &moved).await?;
        let pages: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT page_id FROM notes_blocks
             WHERE id IN (SELECT value FROM json_each(?1)) AND page_id <> ''",
        )
        .bind(keys_json(&moved)?)
        .fetch_all(&mut *conn)
        .await
        .map_err(|error| format!("load pages of repaired Notes blocks: {error}"))?;
        crate::sync_apply::queue_page_history(conn, &pages).await?;
    }
    Ok(())
}

/// The page a cycle moves to: the known page of `chosen`, else of the smallest member with one.
async fn cycle_page(
    conn: &mut SqliteConnection,
    placements: &BTreeMap<String, Placement>,
    chosen: &str,
    members: &[String],
) -> Result<Option<String>, String> {
    let mut candidates: Vec<&str> = vec![chosen];
    let mut rest: Vec<&str> = members.iter().map(String::as_str).collect();
    rest.sort_unstable();
    candidates.extend(rest.into_iter().filter(|member| *member != chosen));
    for candidate in candidates {
        let Some(page) = placements
            .get(candidate)
            .map(|row| row.page_id.as_str())
            .filter(|page| !page.is_empty())
        else {
            continue;
        };
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM notes_pages WHERE id = ?)")
                .bind(page)
                .fetch_one(&mut *conn)
                .await
                .map_err(|error| format!("check Notes repair page: {error}"))?;
        if exists {
            return Ok(Some(page.to_string()));
        }
    }
    Ok(None)
}

/// Makes `block_id` a root block of `page_id`, placed after the page's last root block.
async fn move_to_page_root(
    conn: &mut SqliteConnection,
    placements: &mut BTreeMap<String, Placement>,
    block_id: &str,
    page_id: &str,
) -> Result<(), String> {
    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT order_key FROM notes_blocks WHERE parent_page_id = ? AND id <> ?",
    )
    .bind(page_id)
    .bind(block_id)
    .fetch_all(&mut *conn)
    .await
    .map_err(|error| format!("load Notes root order keys: {error}"))?;
    let last = keys
        .iter()
        .filter_map(|key| OrderKey::parse(key).ok())
        .max();
    let order_key = match last {
        Some(last) => last
            .key_after()
            .map_err(|error| format!("order repaired Notes block: {error}"))?,
        None => OrderKey::first(),
    };
    sqlx::query(
        "UPDATE notes_blocks
         SET parent_type = 'page_id',
             parent_page_id = ?1,
             parent_block_id = NULL,
             page_id = ?1,
             order_key = ?2,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?3",
    )
    .bind(page_id)
    .bind(order_key.as_str())
    .bind(block_id)
    .execute(&mut *conn)
    .await
    .map_err(|error| format!("move repaired Notes block: {error}"))?;
    if let Some(row) = placements.get_mut(block_id) {
        row.parent_page_id = Some(page_id.to_string());
        row.parent_block_id = None;
        row.page_id = page_id.to_string();
    }
    Ok(())
}

/// Moves the smallest page of every nesting cycle that touches the changed rows to the
/// workspace and deletes its `child_page` block.
async fn repair_page_nesting(
    conn: &mut SqliteConnection,
    page_ids: &[String],
    block_ids: &[String],
    report: &mut StructureRepair,
) -> Result<(), String> {
    let mut seeds: Vec<String> = page_ids.to_vec();
    seeds.extend(block_ids.iter().cloned());
    if seeds.is_empty() {
        return Ok(());
    }
    let containers: Vec<(String, Option<String>)> = sqlx::query_as(
        "WITH RECURSIVE
         seed(id) AS (
             SELECT value FROM json_each(?1)
             UNION
             SELECT page_id FROM notes_blocks WHERE id IN (SELECT value FROM json_each(?1))
         ),
         up(id) AS (
             SELECT id FROM notes_pages WHERE id IN seed
             UNION
             SELECT block.page_id
             FROM notes_pages AS page
             JOIN up ON page.id = up.id
             JOIN notes_blocks AS block ON block.id = page.child_block_id
             WHERE block.page_id <> ''
         )
         SELECT page.id, NULLIF(block.page_id, '')
         FROM notes_pages AS page
         LEFT JOIN notes_blocks AS block ON block.id = page.child_block_id
         WHERE page.id IN up",
    )
    .bind(keys_json(&seeds)?)
    .fetch_all(&mut *conn)
    .await
    .map_err(|error| format!("load Notes page nesting for repair: {error}"))?;
    let mut container: BTreeMap<String, Option<String>> = containers.into_iter().collect();
    let pages: Vec<String> = container.keys().cloned().collect();
    for page in pages {
        let mut seen: Vec<String> = Vec::new();
        let mut current = Some(page);
        while let Some(id) = current {
            if let Some(at) = seen.iter().position(|item| *item == id) {
                if let Some(chosen) = seen[at..].iter().min().cloned() {
                    detach_page(conn, &chosen).await?;
                    container.insert(chosen.clone(), None);
                    report.detached_pages.push(chosen);
                }
                break;
            }
            seen.push(id.clone());
            current = container.get(&id).cloned().flatten();
        }
    }
    Ok(())
}

/// Moves `page_id` to the workspace and deletes the `child_page` block that nested it.
async fn detach_page(conn: &mut SqliteConnection, page_id: &str) -> Result<(), String> {
    let block: Option<String> =
        sqlx::query_scalar("SELECT child_block_id FROM notes_pages WHERE id = ?")
            .bind(page_id)
            .fetch_optional(&mut *conn)
            .await
            .map_err(|error| format!("load nested Notes page: {error}"))?
            .flatten();
    sqlx::query(
        "UPDATE notes_pages
         SET child_block_id = NULL,
             parent_type = 'workspace',
             parent_page_id = NULL,
             parent_block_id = NULL,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(page_id)
    .execute(&mut *conn)
    .await
    .map_err(|error| format!("detach nested Notes page: {error}"))?;
    if let Some(block) = block {
        sqlx::query("DELETE FROM notes_blocks WHERE id = ? AND type = 'child_page'")
            .bind(&block)
            .execute(&mut *conn)
            .await
            .map_err(|error| format!("delete detached Notes page block: {error}"))?;
    }
    crate::sync_apply::queue_page_history(conn, &[page_id.to_string()]).await
}
