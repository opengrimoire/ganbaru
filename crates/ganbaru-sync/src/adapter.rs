//! Domain adapters: composite value codecs, intrinsic validation, derived projections, coupled
//! priorities, asset references, and recovery presentation for tables whose manifest names an
//! adapter.

use std::fmt;

use ganbaru_sync_contracts::{Field, GroupId, GroupValue, TableId, Value};

use crate::assets::AssetRef;

/// A value the adapter refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdapterError(pub &'static str);

impl fmt::Display for AdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid {}", self.0)
    }
}

impl std::error::Error for AdapterError {}

/// Title and preview of a row offered for recovery.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Presentation {
    /// Short title, possibly empty.
    pub title: String,
    /// Plain text preview, possibly empty.
    pub preview: String,
}

/// Domain logic the engine cannot derive from the manifest.
///
/// Every method is a pure function of its inputs, so every replica derives the same values.
pub trait DomainAdapter: Send + Sync {
    /// Name the manifest refers to.
    fn name(&self) -> &'static str;

    /// Encodes the rows of a custom codec owned table, each given as fields in
    /// [`crate::manifest::OwnedTable`] column order, into its blob field. The encoding must not
    /// normalize: a list it cannot carry unchanged is refused, so every accepted blob decodes to
    /// the rows it came from and encodes back to the same bytes.
    fn encode_owned(
        &self,
        table: TableId,
        owned: &str,
        rows: Vec<Vec<Field>>,
    ) -> Result<Vec<u8>, AdapterError> {
        let _ = (table, owned, rows);
        Err(AdapterError("owned table"))
    }

    /// Decodes the blob field of a custom codec owned table into its rows in order.
    fn decode_owned(
        &self,
        table: TableId,
        owned: &str,
        bytes: &[u8],
    ) -> Result<Vec<Vec<Field>>, AdapterError> {
        let _ = (table, owned, bytes);
        Err(AdapterError("owned table"))
    }

    /// Row lists of a custom codec owned table that the codec round-trip conformance encodes,
    /// stores, reads back, and decodes. They should reach the codec's bounds.
    fn owned_samples(&self, table: TableId, owned: &str) -> Vec<Vec<Vec<Field>>> {
        let _ = (table, owned);
        Vec::new()
    }

    /// Groups of a table whose values may reference managed assets, read by
    /// [`crate::Engine::referenced_assets`].
    fn asset_groups(&self, table: TableId) -> &'static [GroupId] {
        let _ = table;
        &[]
    }

    /// Assets a group value references, each in a class of the table's domain. Called for the
    /// groups [`DomainAdapter::asset_groups`] names, with live and retained values.
    fn asset_refs(
        &self,
        table: TableId,
        group: GroupId,
        value: &Value,
    ) -> Result<Vec<AssetRef>, AdapterError> {
        let _ = (table, group, value);
        Ok(Vec::new())
    }

    /// Derived column values to set when the group materializes.
    fn derived(
        &self,
        table: TableId,
        group: GroupId,
        value: &Value,
    ) -> Result<Vec<(&'static str, Field)>, AdapterError>;

    /// Priority of a coupled group value; greater wins before clocks are compared.
    fn priority(&self, table: TableId, group: GroupId, value: &Value) -> u8;

    /// Checks rules the column types cannot express. Called for every group of the table.
    fn validate(&self, table: TableId, group: GroupId, value: &Value) -> Result<(), AdapterError>;

    /// Checks rules that span groups of one row. Called after [`DomainAdapter::validate`] for a
    /// change that writes every group, with the groups in ascending order.
    fn validate_row(&self, table: TableId, groups: &[GroupValue]) -> Result<(), AdapterError> {
        let _ = (table, groups);
        Ok(())
    }

    /// Presentation of a retained row from its winning group values.
    fn presentation(&self, table: TableId, values: &[(GroupId, Value)]) -> Presentation;
}

/// Registered adapters by name.
#[derive(Default)]
pub struct Adapters {
    adapters: Vec<Box<dyn DomainAdapter>>,
}

impl Adapters {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an adapter, replacing one with the same name.
    pub fn with(mut self, adapter: Box<dyn DomainAdapter>) -> Self {
        self.adapters
            .retain(|existing| existing.name() != adapter.name());
        self.adapters.push(adapter);
        self
    }

    /// Names of the registered adapters.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.adapters.iter().map(|adapter| adapter.name())
    }

    /// Adapter by name.
    pub fn get(&self, name: &str) -> Option<&dyn DomainAdapter> {
        self.adapters
            .iter()
            .find(|adapter| adapter.name() == name)
            .map(Box::as_ref)
    }
}

impl fmt::Debug for Adapters {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_list()
            .entries(self.adapters.iter().map(|adapter| adapter.name()))
            .finish()
    }
}
