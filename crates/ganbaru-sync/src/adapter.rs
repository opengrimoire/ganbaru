//! Domain adapters: composite value codecs, intrinsic validation, derived projections, coupled
//! priorities, and recovery presentation for tables whose manifest names an adapter.

use std::fmt;

use ganbaru_sync_contracts::{Field, GroupId, TableId, Value};

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

    /// Encodes owned child rows, each given as fields in [`crate::manifest::OwnedTable`] column
    /// order, into the group value. The encoding must not normalize: a non-canonical list must
    /// fail [`DomainAdapter::validate`] instead of changing on the way through.
    fn encode_owned(
        &self,
        table: TableId,
        group: GroupId,
        rows: Vec<Vec<Field>>,
    ) -> Result<Value, AdapterError>;

    /// Decodes a group value into owned child rows in order.
    fn decode_owned(
        &self,
        table: TableId,
        group: GroupId,
        value: &Value,
    ) -> Result<Vec<Vec<Field>>, AdapterError>;

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
