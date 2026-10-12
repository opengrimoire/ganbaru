//! Order key placement for Quick notes and tags.
//!
//! Rows sort by `(order_key, id)`. Concurrent writers can produce equal keys, so placement
//! treats a run of keys equal to the lower neighbor as part of the gap and rewrites it.

use ganbaru_sync_contracts::{OrderKey, OrderKeyError};
use sqlx::FromRow;

/// One row of an ordered group, in display order.
#[derive(Clone, Debug, PartialEq, Eq, FromRow)]
pub(super) struct OrderedRow {
    pub id: String,
    pub order_key: String,
}

/// Order keys to write so `id` sits at `index` among `others`, which exclude `id` and are
/// sorted by `(order_key, id)`.
///
/// The result usually holds only `id`. Rows after the gap whose key equals the lower neighbor
/// are rewritten too, and when the key space between the neighbors is exhausted the whole
/// group is re-keyed with rank keys.
pub(super) fn placement(
    others: &[OrderedRow],
    index: usize,
    id: &str,
) -> Result<Vec<(String, OrderKey)>, String> {
    if index > others.len() {
        return Err("order placement index is outside the group".to_string());
    }
    match place_in_gap(others, index, id) {
        Ok(updates) => Ok(updates),
        Err(OrderKeyError::Exhausted) => rekey(others, index, id),
        Err(error) => Err(format!("place order key: {error}")),
    }
}

/// Splits the key for `id` from the keys of the other rows.
pub(super) fn take_key(
    mut updates: Vec<(String, OrderKey)>,
    id: &str,
) -> Result<(OrderKey, Vec<(String, OrderKey)>), String> {
    let position = updates
        .iter()
        .position(|(row_id, _)| row_id == id)
        .ok_or_else(|| "order placement omitted the placed row".to_string())?;
    let (_, key) = updates.swap_remove(position);
    Ok((key, updates))
}

fn parse(row: &OrderedRow) -> Result<OrderKey, OrderKeyError> {
    OrderKey::parse(&row.order_key)
}

fn place_in_gap(
    others: &[OrderedRow],
    index: usize,
    id: &str,
) -> Result<Vec<(String, OrderKey)>, OrderKeyError> {
    let low = index
        .checked_sub(1)
        .map(|lower| parse(&others[lower]))
        .transpose()?;
    let mut end = index;
    if let Some(low) = &low {
        while others
            .get(end)
            .is_some_and(|row| row.order_key == low.as_str())
        {
            end += 1;
        }
    }
    let high = others.get(end).map(parse).transpose()?;
    let mut current = low;
    let mut updates = Vec::with_capacity(end - index + 1);
    for row_id in std::iter::once(id).chain(others[index..end].iter().map(|row| row.id.as_str())) {
        let key = OrderKey::between(current.as_ref(), high.as_ref())?;
        updates.push((row_id.to_string(), key.clone()));
        current = Some(key);
    }
    Ok(updates)
}

fn rekey(others: &[OrderedRow], index: usize, id: &str) -> Result<Vec<(String, OrderKey)>, String> {
    let ordered = others[..index]
        .iter()
        .map(|row| (row.id.as_str(), Some(row.order_key.as_str())))
        .chain(std::iter::once((id, None)))
        .chain(
            others[index..]
                .iter()
                .map(|row| (row.id.as_str(), Some(row.order_key.as_str()))),
        );
    let mut updates = Vec::new();
    for (position, (row_id, current)) in ordered.enumerate() {
        let position =
            u32::try_from(position).map_err(|_| "order group is too large".to_string())?;
        let key =
            OrderKey::rank(position).map_err(|error| format!("re-key order group: {error}"))?;
        if current != Some(key.as_str()) {
            updates.push((row_id.to_string(), key));
        }
    }
    Ok(updates)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(keys: &[(&str, &str)]) -> Vec<OrderedRow> {
        keys.iter()
            .map(|(id, order_key)| OrderedRow {
                id: (*id).to_string(),
                order_key: (*order_key).to_string(),
            })
            .collect()
    }

    /// Applies updates and returns ids in `(order_key, id)` order.
    fn apply(others: &[OrderedRow], id: &str, updates: &[(String, OrderKey)]) -> Vec<String> {
        let mut all = others.to_vec();
        all.push(OrderedRow {
            id: id.to_string(),
            order_key: String::new(),
        });
        for (row_id, key) in updates {
            let row = all.iter_mut().find(|row| &row.id == row_id).unwrap();
            row.order_key = key.as_str().to_string();
        }
        assert!(
            all.iter()
                .all(|row| OrderKey::parse(&row.order_key).is_ok())
        );
        all.sort_by(|left, right| {
            (left.order_key.as_str(), left.id.as_str())
                .cmp(&(right.order_key.as_str(), right.id.as_str()))
        });
        all.into_iter().map(|row| row.id).collect()
    }

    #[test]
    fn places_into_empty_and_open_ended_groups() {
        assert_eq!(apply(&[], "n", &placement(&[], 0, "n").unwrap()), ["n"]);
        let group = rows(&[("a", "a0"), ("b", "a1")]);
        for (index, expected) in [
            (0, ["n", "a", "b"]),
            (1, ["a", "n", "b"]),
            (2, ["a", "b", "n"]),
        ] {
            let updates = placement(&group, index, "n").unwrap();
            assert_eq!(updates.len(), 1);
            assert_eq!(apply(&group, "n", &updates), expected);
        }
    }

    #[test]
    fn rewrites_rows_that_share_the_lower_key() {
        let group = rows(&[("a", "a0"), ("b", "a0"), ("c", "a0"), ("d", "a1")]);
        let updates = placement(&group, 1, "n").unwrap();
        assert_eq!(
            updates
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            ["n", "b", "c"]
        );
        assert_eq!(apply(&group, "n", &updates), ["a", "n", "b", "c", "d"]);

        let tail = rows(&[("a", "a0"), ("b", "a0")]);
        assert_eq!(
            apply(&tail, "n", &placement(&tail, 1, "n").unwrap()),
            ["a", "n", "b"]
        );
    }

    #[test]
    fn rekeys_the_group_when_the_gap_is_exhausted() {
        let low = format!("a0{}1", "0".repeat(125));
        let high = format!("a0{}2", "0".repeat(125));
        let group = rows(&[("a", "a0"), ("b", &low), ("c", &high)]);
        let updates = placement(&group, 2, "n").unwrap();
        assert!(updates.len() > 1);
        assert_eq!(apply(&group, "n", &updates), ["a", "b", "n", "c"]);
    }

    #[test]
    fn rejects_invalid_indexes_and_keys() {
        let group = rows(&[("a", "a0")]);
        assert!(placement(&group, 2, "n").is_err());
        assert!(placement(&rows(&[("a", "a00")]), 1, "n").is_err());
    }

    #[test]
    fn repeated_insertion_into_one_gap_stays_ordered() {
        let mut group = rows(&[("first", "a0"), ("last", "a1")]);
        for step in 0..1000 {
            let id = format!("n{step:04}");
            let updates = placement(&group, 1, &id).unwrap();
            let order = apply(&group, &id, &updates);
            assert_eq!(order[0], "first");
            assert_eq!(order[1], id);
            assert_eq!(order.last().unwrap(), "last");
            for (row_id, key) in updates {
                match group.iter_mut().find(|row| row.id == row_id) {
                    Some(row) => row.order_key = key.into_string(),
                    None => group.push(OrderedRow {
                        id: row_id,
                        order_key: key.into_string(),
                    }),
                }
            }
            group.sort_by(|left, right| {
                (left.order_key.as_str(), left.id.as_str())
                    .cmp(&(right.order_key.as_str(), right.id.as_str()))
            });
        }
    }
}
