//! Version vectors: for each writer, the highest sequence included.

use crate::bounds::MAX_DEPENDENCIES;
use crate::codec::{CodecError, Reader, Writer};
use crate::ids::{ID_BYTES, Seq, WriterId};
use std::collections::BTreeMap;

/// Highest included sequence per writer. Writers at zero are absent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionVector(BTreeMap<WriterId, Seq>);

impl VersionVector {
    /// The empty vector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Highest included sequence of a writer, zero when absent.
    pub fn get(&self, writer: &WriterId) -> Seq {
        self.0.get(writer).copied().unwrap_or(0)
    }

    /// Sets a writer's entry, removing it at zero.
    pub fn set(&mut self, writer: WriterId, seq: Seq) {
        if seq == 0 {
            self.0.remove(&writer);
        } else {
            self.0.insert(writer, seq);
        }
    }

    /// Raises a writer's entry to at least `seq`.
    pub fn advance(&mut self, writer: WriterId, seq: Seq) {
        if seq > self.get(&writer) {
            self.0.insert(writer, seq);
        }
    }

    /// Whether the operation `(writer, seq)` is included.
    pub fn covers(&self, writer: &WriterId, seq: Seq) -> bool {
        seq <= self.get(writer)
    }

    /// Raises every entry to the maximum of both vectors.
    pub fn merge(&mut self, other: &Self) {
        for (writer, seq) in &other.0 {
            self.advance(*writer, *seq);
        }
    }

    /// Whether every operation included in `other` is included here.
    pub fn dominates(&self, other: &Self) -> bool {
        other
            .0
            .iter()
            .all(|(writer, seq)| self.covers(writer, *seq))
    }

    /// Entries that are higher here than in `previous`, in writer order.
    pub fn delta_since(&self, previous: &Self) -> Vec<(WriterId, Seq)> {
        self.0
            .iter()
            .filter(|(writer, seq)| **seq > previous.get(writer))
            .map(|(writer, seq)| (*writer, *seq))
            .collect()
    }

    /// Entries in writer order.
    pub fn iter(&self) -> impl Iterator<Item = (WriterId, Seq)> + '_ {
        self.0.iter().map(|(writer, seq)| (*writer, *seq))
    }

    /// Number of writers with a nonzero entry.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether every entry is zero.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Canonical encoding: a u32 count, then writer id and u64 sequence in writer order.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::with_capacity(4 + self.0.len() * (ID_BYTES + 8));
        writer.u32(self.0.len() as u32);
        for (id, seq) in &self.0 {
            writer.raw(id.as_bytes());
            writer.u64(*seq);
        }
        writer.into_bytes()
    }

    /// Decodes the canonical encoding, refusing unsorted, duplicate, or zero entries.
    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        let mut reader = Reader::new(bytes);
        let count = reader.u32()? as usize;
        if count > reader.remaining() / (ID_BYTES + 8) {
            return Err(CodecError::Truncated);
        }
        let mut vector = Self::new();
        let mut previous: Option<WriterId> = None;
        for _ in 0..count {
            let writer = WriterId::from_bytes(reader.array()?);
            let seq = reader.u64()?;
            if seq == 0 || previous.is_some_and(|last| last >= writer) {
                return Err(CodecError::Malformed("version vector"));
            }
            vector.0.insert(writer, seq);
            previous = Some(writer);
        }
        reader.finish()?;
        Ok(vector)
    }
}

impl FromIterator<(WriterId, Seq)> for VersionVector {
    fn from_iter<I: IntoIterator<Item = (WriterId, Seq)>>(entries: I) -> Self {
        let mut vector = Self::new();
        for (writer, seq) in entries {
            vector.advance(writer, seq);
        }
        vector
    }
}

/// Validates a dependency list: at most [`MAX_DEPENDENCIES`] entries, strictly ascending by
/// writer, sequences from 1, and never the sealing writer itself.
pub fn validate_dependencies(
    dependencies: &[(WriterId, Seq)],
    own_writer: &WriterId,
) -> Result<(), CodecError> {
    if dependencies.len() > MAX_DEPENDENCIES {
        return Err(CodecError::Bounds("dependencies"));
    }
    let mut previous: Option<&WriterId> = None;
    for (writer, seq) in dependencies {
        if *seq == 0 || writer == own_writer || previous.is_some_and(|last| last >= writer) {
            return Err(CodecError::Malformed("dependencies"));
        }
        previous = Some(writer);
    }
    Ok(())
}
