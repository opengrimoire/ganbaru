//! Order key placement for Quick notes and tags, over the shared placement in
//! `ganbaru-sync-contracts`.

use ganbaru_sync_contracts::{OrderKey, order_placement};
use sqlx::FromRow;

/// One row of an ordered group, in display order.
#[derive(Clone, Debug, PartialEq, Eq, FromRow)]
pub(super) struct OrderedRow {
    pub id: String,
    pub order_key: String,
}

impl order_placement::OrderedRow for OrderedRow {
    fn id(&self) -> &str {
        &self.id
    }

    fn order_key(&self) -> &str {
        &self.order_key
    }
}

/// Order keys to write so `id` sits at `index` among `others`, which exclude `id` and are
/// sorted by `(order_key, id)`.
pub(super) fn placement(
    others: &[OrderedRow],
    index: usize,
    id: &str,
) -> Result<Vec<(String, OrderKey)>, String> {
    order_placement::placement(others, index, id).map_err(|error| error.to_string())
}

/// Splits the key for `id` from the keys of the other rows.
pub(super) fn take_key(
    updates: Vec<(String, OrderKey)>,
    id: &str,
) -> Result<(OrderKey, Vec<(String, OrderKey)>), String> {
    order_placement::take_key(updates, id).map_err(|error| error.to_string())
}
