//! Order key placement within an ordered group.
//!
//! Rows sort by `(order_key, id)`. Concurrent writers can produce equal keys, so placement
//! treats a run of keys equal to the lower neighbor as part of the gap and rewrites it.

use crate::order_key::{OrderKey, OrderKeyError};
use std::fmt;

/// One row of an ordered group.
pub trait OrderedRow {
    /// Row id, the tie breaker after the order key.
    fn id(&self) -> &str;
    /// Stored order key text.
    fn order_key(&self) -> &str;
}

impl OrderedRow for (String, String) {
    fn id(&self) -> &str {
        &self.0
    }

    fn order_key(&self) -> &str {
        &self.1
    }
}

/// A placement that cannot be computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderPlacementError {
    /// The target index is past the end of the group.
    IndexOutOfRange,
    /// The group holds more rows than rank keys can number.
    GroupTooLarge,
    /// The placement result does not contain the placed row.
    PlacedRowMissing,
    /// A requested order does not list each row of the group exactly once.
    RowMismatch,
    /// A stored key is invalid or a key cannot be generated.
    Key(OrderKeyError),
    /// Re-keying the group failed.
    Rekey(OrderKeyError),
}

impl fmt::Display for OrderPlacementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IndexOutOfRange => {
                formatter.write_str("order placement index is outside the group")
            }
            Self::GroupTooLarge => formatter.write_str("order group is too large"),
            Self::PlacedRowMissing => formatter.write_str("order placement omitted the placed row"),
            Self::RowMismatch => {
                formatter.write_str("requested order does not list each row of the group once")
            }
            Self::Key(error) => write!(formatter, "place order key: {error}"),
            Self::Rekey(error) => write!(formatter, "re-key order group: {error}"),
        }
    }
}

impl std::error::Error for OrderPlacementError {}

/// Order keys to write so `id` sits at `index` among `others`, which exclude `id` and are
/// sorted by `(order_key, id)`.
///
/// The result usually holds only `id`. Rows after the gap whose key equals the lower neighbor
/// are rewritten too, and when the key space between the neighbors is exhausted the whole
/// group is re-keyed with rank keys.
pub fn placement<R: OrderedRow>(
    others: &[R],
    index: usize,
    id: &str,
) -> Result<Vec<(String, OrderKey)>, OrderPlacementError> {
    if index > others.len() {
        return Err(OrderPlacementError::IndexOutOfRange);
    }
    match place_in_gap(others, index, id) {
        Ok(updates) => Ok(updates),
        Err(OrderKeyError::Exhausted) => rekey(others, index, id),
        Err(error) => Err(OrderPlacementError::Key(error)),
    }
}

/// Order keys to write so the group sorts as `desired`.
///
/// `current` holds every row of the group sorted by `(order_key, id)`, and `desired` lists the
/// same ids in the requested order. Rows in the longest run of `desired` that already sorts
/// correctly keep their keys; every other row is placed into its gap in turn, so a single move
/// writes one key. The result holds only rows whose key changes.
pub fn reorder<R: OrderedRow>(
    current: &[R],
    desired: &[&str],
) -> Result<Vec<(String, OrderKey)>, OrderPlacementError> {
    let ranks = current
        .iter()
        .enumerate()
        .map(|(rank, row)| (row.id(), rank))
        .collect::<std::collections::HashMap<_, _>>();
    if desired.len() != current.len() {
        return Err(OrderPlacementError::RowMismatch);
    }
    let sequence = desired
        .iter()
        .map(|id| {
            ranks
                .get(id)
                .copied()
                .ok_or(OrderPlacementError::RowMismatch)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = vec![false; current.len()];
    for rank in &sequence {
        if std::mem::replace(&mut seen[*rank], true) {
            return Err(OrderPlacementError::RowMismatch);
        }
    }
    let kept = longest_increasing_run(&sequence);
    let mut group = Vec::<(String, String)>::with_capacity(desired.len());
    for (index, rank) in sequence.iter().enumerate() {
        if kept[index] {
            let row = &current[*rank];
            group.push((row.id().to_string(), row.order_key().to_string()));
        }
    }
    let mut position_in_group = 0;
    for (index, id) in desired.iter().enumerate() {
        if kept[index] {
            position_in_group += 1;
            continue;
        }
        let updates = placement(&group, position_in_group, id)?;
        let (key, others) = take_key(updates, id)?;
        for (row_id, row_key) in others {
            if let Some(row) = group.iter_mut().find(|row| row.0 == row_id) {
                row.1 = row_key.into_string();
            }
        }
        group.insert(position_in_group, ((*id).to_string(), key.into_string()));
        position_in_group += 1;
    }
    group
        .into_iter()
        .filter(|(id, key)| current[ranks[id.as_str()]].order_key() != key)
        .map(|(id, key)| OrderKey::parse(&key).map(|key| (id, key)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(OrderPlacementError::Key)
}

/// Marks the entries of one longest strictly increasing subsequence of `sequence`.
fn longest_increasing_run(sequence: &[usize]) -> Vec<bool> {
    let mut tails: Vec<usize> = Vec::new();
    let mut previous = vec![None; sequence.len()];
    for (index, value) in sequence.iter().enumerate() {
        let slot = tails.partition_point(|tail| sequence[*tail] < *value);
        if slot > 0 {
            previous[index] = Some(tails[slot - 1]);
        }
        if slot == tails.len() {
            tails.push(index);
        } else {
            tails[slot] = index;
        }
    }
    let mut kept = vec![false; sequence.len()];
    let mut cursor = tails.last().copied();
    while let Some(index) = cursor {
        kept[index] = true;
        cursor = previous[index];
    }
    kept
}

/// Splits the key for `id` from the keys of the other rows.
pub fn take_key(
    mut updates: Vec<(String, OrderKey)>,
    id: &str,
) -> Result<(OrderKey, Vec<(String, OrderKey)>), OrderPlacementError> {
    let position = updates
        .iter()
        .position(|(row_id, _)| row_id == id)
        .ok_or(OrderPlacementError::PlacedRowMissing)?;
    let (_, key) = updates.swap_remove(position);
    Ok((key, updates))
}

fn parse<R: OrderedRow>(row: &R) -> Result<OrderKey, OrderKeyError> {
    OrderKey::parse(row.order_key())
}

fn place_in_gap<R: OrderedRow>(
    others: &[R],
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
            .is_some_and(|row| row.order_key() == low.as_str())
        {
            end += 1;
        }
    }
    let high = others.get(end).map(parse).transpose()?;
    let mut current = low;
    let mut updates = Vec::with_capacity(end - index + 1);
    for row_id in std::iter::once(id).chain(others[index..end].iter().map(|row| row.id())) {
        let key = OrderKey::between(current.as_ref(), high.as_ref())?;
        updates.push((row_id.to_string(), key.clone()));
        current = Some(key);
    }
    Ok(updates)
}

fn rekey<R: OrderedRow>(
    others: &[R],
    index: usize,
    id: &str,
) -> Result<Vec<(String, OrderKey)>, OrderPlacementError> {
    let ordered = others[..index]
        .iter()
        .map(|row| (row.id(), Some(row.order_key())))
        .chain(std::iter::once((id, None)))
        .chain(
            others[index..]
                .iter()
                .map(|row| (row.id(), Some(row.order_key()))),
        );
    let mut updates = Vec::new();
    for (position, (row_id, current)) in ordered.enumerate() {
        let position = u32::try_from(position).map_err(|_| OrderPlacementError::GroupTooLarge)?;
        let key = OrderKey::rank(position).map_err(OrderPlacementError::Rekey)?;
        if current != Some(key.as_str()) {
            updates.push((row_id.to_string(), key));
        }
    }
    Ok(updates)
}

#[cfg(test)]
mod tests {
    use super::*;

    type Row = (String, String);

    fn rows(keys: &[(&str, &str)]) -> Vec<Row> {
        keys.iter()
            .map(|(id, order_key)| ((*id).to_string(), (*order_key).to_string()))
            .collect()
    }

    /// Applies updates and returns ids in `(order_key, id)` order.
    fn apply(others: &[Row], id: &str, updates: &[(String, OrderKey)]) -> Vec<String> {
        let mut all = others.to_vec();
        all.push((id.to_string(), String::new()));
        for (row_id, key) in updates {
            let row = all.iter_mut().find(|row| &row.0 == row_id).unwrap();
            row.1 = key.as_str().to_string();
        }
        assert!(all.iter().all(|row| OrderKey::parse(&row.1).is_ok()));
        all.sort_by(|left, right| {
            (left.1.as_str(), left.0.as_str()).cmp(&(right.1.as_str(), right.0.as_str()))
        });
        all.into_iter().map(|row| row.0).collect()
    }

    #[test]
    fn places_into_empty_and_open_ended_groups() {
        assert_eq!(
            apply(&[], "n", &placement::<Row>(&[], 0, "n").unwrap()),
            ["n"]
        );
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
        assert_eq!(
            placement(&group, 2, "n"),
            Err(OrderPlacementError::IndexOutOfRange)
        );
        assert_eq!(
            placement(&rows(&[("a", "a00")]), 1, "n"),
            Err(OrderPlacementError::Key(OrderKeyError::Invalid))
        );
    }

    #[test]
    fn takes_the_placed_key_apart_from_rewritten_neighbors() {
        let group = rows(&[("a", "a0"), ("b", "a0")]);
        let (key, others) = take_key(placement(&group, 1, "n").unwrap(), "n").unwrap();
        assert!(key.as_str() > "a0");
        assert_eq!(others.len(), 1);
        assert_eq!(others[0].0, "b");
        assert_eq!(
            take_key(Vec::new(), "n"),
            Err(OrderPlacementError::PlacedRowMissing)
        );
    }

    /// Applies reorder updates to the whole group and returns ids in `(order_key, id)` order.
    fn apply_reorder(group: &[Row], updates: &[(String, OrderKey)]) -> Vec<String> {
        let mut all = group.to_vec();
        for (row_id, key) in updates {
            let row = all.iter_mut().find(|row| &row.0 == row_id).unwrap();
            row.1 = key.as_str().to_string();
        }
        all.sort_by(|left, right| {
            (left.1.as_str(), left.0.as_str()).cmp(&(right.1.as_str(), right.0.as_str()))
        });
        all.into_iter().map(|row| row.0).collect()
    }

    #[test]
    fn reorder_writes_one_key_for_a_single_move() {
        let group = rows(&[("a", "a0"), ("b", "a1"), ("c", "a2"), ("d", "a3")]);
        let updates = reorder(&group, &["b", "c", "a", "d"]).unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].0, "a");
        assert_eq!(apply_reorder(&group, &updates), ["b", "c", "a", "d"]);
        assert!(reorder(&group, &["a", "b", "c", "d"]).unwrap().is_empty());
    }

    #[test]
    fn reorder_reaches_any_permutation_including_tied_keys() {
        let group = rows(&[
            ("a", "a0"),
            ("b", "a0"),
            ("c", "a1"),
            ("d", "a1"),
            ("e", "a2"),
        ]);
        for desired in [
            ["e", "d", "c", "b", "a"],
            ["b", "a", "d", "c", "e"],
            ["c", "a", "e", "b", "d"],
        ] {
            let updates = reorder(&group, &desired).unwrap();
            assert_eq!(apply_reorder(&group, &updates), desired);
        }
    }

    #[test]
    fn reorder_rejects_requests_that_do_not_match_the_group() {
        let group = rows(&[("a", "a0"), ("b", "a1")]);
        for desired in [&["a"][..], &["a", "a"], &["a", "x"], &["a", "b", "c"]] {
            assert_eq!(
                reorder(&group, desired),
                Err(OrderPlacementError::RowMismatch)
            );
        }
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
                match group.iter_mut().find(|row| row.0 == row_id) {
                    Some(row) => row.1 = key.into_string(),
                    None => group.push((row_id, key.into_string())),
                }
            }
            group.sort_by(|left, right| {
                (left.1.as_str(), left.0.as_str()).cmp(&(right.1.as_str(), right.0.as_str()))
            });
        }
    }
}
