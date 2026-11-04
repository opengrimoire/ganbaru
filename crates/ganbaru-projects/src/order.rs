//! Order keys for ordered Projects rows.
//!
//! Rows of one ordered group sort by `(order_key, id)`. Commands name a placement relative to a sibling, and the
//! keys are computed here with the shared placement in `ganbaru-sync-contracts`, so a move usually writes one key.

use ganbaru_sync_contracts::{OrderKey, order_placement};
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};

/// Where a created or moved row goes within its ordered group.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum OrderPlacement {
    /// Directly before the sibling with this id.
    Before { id: String },
    /// Directly after the sibling with this id.
    After { id: String },
    /// After every sibling.
    #[default]
    End,
}

/// One ordered group: the rows of `table` whose `conditions` columns equal the given values, sorted by `column`.
pub(crate) struct OrderGroup<'a> {
    pub table: &'static str,
    pub column: &'static str,
    /// Column and value pairs compared with `IS`, so `None` selects rows where the column is null.
    pub conditions: Vec<(&'static str, Option<&'a str>)>,
}

impl<'a> OrderGroup<'a> {
    pub fn new(table: &'static str, column: &'static str) -> Self {
        Self {
            table,
            column,
            conditions: Vec::new(),
        }
    }

    pub fn filter(mut self, column: &'static str, value: Option<&'a str>) -> Self {
        self.conditions.push((column, value));
        self
    }

    /// Every row of the group as `(id, order_key)`, sorted by `(order_key, id)`.
    pub async fn rows(&self, conn: &mut SqliteConnection) -> Result<Vec<(String, String)>, String> {
        let mut query = QueryBuilder::<Sqlite>::new(format!(
            "SELECT id, {} FROM {} WHERE 1 = 1",
            self.column, self.table
        ));
        for (column, value) in &self.conditions {
            query.push(format!(" AND {column} IS ")).push_bind(*value);
        }
        query.push(format!(" ORDER BY {}, id", self.column));
        query
            .build_query_as::<(String, String)>()
            .fetch_all(conn)
            .await
            .map_err(|error| format!("load {} order: {error}", self.table))
    }

    /// Computes the key that puts `id` at `placement` and writes any sibling keys the placement rewrites. The
    /// caller writes the returned key on `id`.
    pub async fn place(
        &self,
        conn: &mut SqliteConnection,
        id: &str,
        placement: &OrderPlacement,
    ) -> Result<OrderKey, String> {
        let (key, rewritten) = self.placement_updates(conn, id, placement).await?;
        self.write(conn, &rewritten).await?;
        Ok(key)
    }

    /// Moves the existing row `id` to `placement`, writing its key and any sibling keys the move rewrites.
    pub async fn move_row(
        &self,
        conn: &mut SqliteConnection,
        id: &str,
        placement: &OrderPlacement,
    ) -> Result<OrderKey, String> {
        let key = self.place(conn, id, placement).await?;
        self.write(conn, &[(id.to_string(), key.clone())]).await?;
        Ok(key)
    }

    /// Moves the existing row `id` to `placement` and returns the ids of every row whose key was written.
    pub async fn move_row_reporting(
        &self,
        conn: &mut SqliteConnection,
        id: &str,
        placement: &OrderPlacement,
    ) -> Result<Vec<String>, String> {
        let (key, mut rewritten) = self.placement_updates(conn, id, placement).await?;
        rewritten.push((id.to_string(), key));
        self.write(conn, &rewritten).await?;
        Ok(rewritten.into_iter().map(|(row_id, _)| row_id).collect())
    }

    /// The key that puts `id` at `placement`, and the sibling keys the placement rewrites.
    async fn placement_updates(
        &self,
        conn: &mut SqliteConnection,
        id: &str,
        placement: &OrderPlacement,
    ) -> Result<(OrderKey, Vec<(String, OrderKey)>), String> {
        let others: Vec<(String, String)> = self
            .rows(conn)
            .await?
            .into_iter()
            .filter(|(row_id, _)| row_id != id)
            .collect();
        let index = placement_index(&others, id, placement)?;
        let updates = order_placement::placement(&others, index, id)
            .map_err(|error| format!("place {} order key: {error}", self.table))?;
        order_placement::take_key(updates, id)
            .map_err(|error| format!("place {} order key: {error}", self.table))
    }

    /// Writes keys so the group sorts as `desired`, which must list every row once, and returns the changed rows.
    pub async fn reorder(
        &self,
        conn: &mut SqliteConnection,
        desired: &[&str],
    ) -> Result<Vec<(String, OrderKey)>, String> {
        let current = self.rows(conn).await?;
        let updates = order_placement::reorder(&current, desired)
            .map_err(|error| format!("reorder {}: {error}", self.table))?;
        self.write(conn, &updates).await?;
        Ok(updates)
    }

    /// Writes order keys of rows in the group without touching any other column. The group conditions are part of
    /// the match, since some ids, such as priority ids, are only unique within their project.
    pub async fn write(
        &self,
        conn: &mut SqliteConnection,
        updates: &[(String, OrderKey)],
    ) -> Result<(), String> {
        for (id, key) in updates {
            let mut query = QueryBuilder::<Sqlite>::new(format!(
                "UPDATE {} SET {} = ",
                self.table, self.column
            ));
            query
                .push_bind(key.as_str())
                .push(" WHERE id = ")
                .push_bind(id);
            for (column, value) in &self.conditions {
                query.push(format!(" AND {column} IS ")).push_bind(*value);
            }
            query
                .build()
                .execute(&mut *conn)
                .await
                .map_err(|error| format!("write {} order key: {error}", self.table))?;
        }
        Ok(())
    }
}

/// Index among `others` that `placement` names. A sibling outside the group is refused.
fn placement_index(
    others: &[(String, String)],
    id: &str,
    placement: &OrderPlacement,
) -> Result<usize, String> {
    let position = |sibling: &str| {
        if sibling.trim() == id {
            return Err("an item cannot be placed relative to itself".to_string());
        }
        others
            .iter()
            .position(|(row_id, _)| row_id == sibling.trim())
            .ok_or_else(|| "placement sibling is not in the same group".to_string())
    };
    match placement {
        OrderPlacement::End => Ok(others.len()),
        OrderPlacement::Before { id: sibling } => position(sibling),
        OrderPlacement::After { id: sibling } => position(sibling).map(|index| index + 1),
    }
}

/// Rank keys for rows created together in a fixed order, such as seeded defaults.
pub(crate) fn rank_key(index: usize) -> Result<OrderKey, String> {
    u32::try_from(index)
        .map_err(|_| "too many seeded rows".to_string())
        .and_then(|index| OrderKey::rank(index).map_err(|error| format!("rank order key: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(ids: &[&str]) -> Vec<(String, String)> {
        ids.iter()
            .enumerate()
            .map(|(index, id)| (id.to_string(), rank_key(index).unwrap().into_string()))
            .collect()
    }

    #[test]
    fn order_key_placement_index_resolves_siblings() {
        let others = rows(&["a", "b", "c"]);
        assert_eq!(
            placement_index(&others, "x", &OrderPlacement::End).unwrap(),
            3
        );
        assert_eq!(
            placement_index(&others, "x", &OrderPlacement::Before { id: "b".into() }).unwrap(),
            1
        );
        assert_eq!(
            placement_index(&others, "x", &OrderPlacement::After { id: "c".into() }).unwrap(),
            3
        );
        assert!(placement_index(&others, "x", &OrderPlacement::After { id: "z".into() }).is_err());
        assert!(placement_index(&others, "a", &OrderPlacement::After { id: "a".into() }).is_err());
    }

    #[test]
    fn order_key_placement_deserializes_tagged_kinds() {
        let before: OrderPlacement = serde_json::from_str(r#"{"kind":"before","id":"a"}"#).unwrap();
        assert_eq!(before, OrderPlacement::Before { id: "a".into() });
        let end: OrderPlacement = serde_json::from_str(r#"{"kind":"end"}"#).unwrap();
        assert_eq!(end, OrderPlacement::End);
        assert!(serde_json::from_str::<OrderPlacement>(r#"{"kind":"middle"}"#).is_err());
    }
}
