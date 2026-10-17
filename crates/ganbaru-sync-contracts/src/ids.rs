//! Space, writer, operation, row, table, and group identifiers.

use crate::bounds::{MAX_GROUPS, MAX_ROW_KEY_BYTES};
use crate::signing::{PERSONAL_SPACE_DOMAIN, WRITER_ID_DOMAIN, WriterPublicKey, domain_digest};
use std::fmt;

/// Length of space and writer ids.
pub const ID_BYTES: usize = 16;

/// Lowercase hexadecimal form of bytes.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    text
}

/// Parses lowercase or uppercase hexadecimal into a fixed array.
pub fn parse_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let bytes = text.as_bytes();
    if bytes.len() != N * 2 {
        return None;
    }
    let mut output = [0u8; N];
    for (index, [high, low]) in bytes.as_chunks::<2>().0.iter().enumerate() {
        let high = char::from(*high).to_digit(16)?;
        let low = char::from(*low).to_digit(16)?;
        output[index] = ((high << 4) | low) as u8;
    }
    Some(output)
}

fn truncated_digest(domain: &[u8], payload: &[u8]) -> [u8; ID_BYTES] {
    let digest = domain_digest(domain, &[payload]);
    let mut id = [0u8; ID_BYTES];
    id.copy_from_slice(&digest[..ID_BYTES]);
    id
}

macro_rules! fixed_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name([u8; ID_BYTES]);

        impl $name {
            /// Wraps raw id bytes.
            pub const fn from_bytes(bytes: [u8; ID_BYTES]) -> Self {
                Self(bytes)
            }

            /// Parses raw id bytes from a slice.
            pub fn from_slice(bytes: &[u8]) -> Option<Self> {
                <[u8; ID_BYTES]>::try_from(bytes).ok().map(Self)
            }

            /// Parses the hexadecimal form.
            pub fn from_hex(text: &str) -> Option<Self> {
                parse_hex(text).map(Self)
            }

            /// Raw id bytes.
            pub const fn as_bytes(&self) -> &[u8; ID_BYTES] {
                &self.0
            }

            /// Lowercase hexadecimal form.
            pub fn to_hex(&self) -> String {
                hex(&self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.debug_tuple(stringify!($name)).field(&self.to_hex()).finish()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.to_hex())
            }
        }
    };
}

fixed_id!(
    /// A replication space. The personal space of a vault derives from its vault id.
    SpaceId
);

fixed_id!(
    /// An installation-scoped writer, derived from its public key.
    WriterId
);

impl SpaceId {
    /// The personal space of a vault.
    pub fn personal(vault_id: &str) -> Self {
        Self(truncated_digest(PERSONAL_SPACE_DOMAIN, vault_id.as_bytes()))
    }
}

impl WriterId {
    /// The writer id certified for a public key.
    pub fn for_public_key(key: &WriterPublicKey) -> Self {
        Self(truncated_digest(WRITER_ID_DOMAIN, key.as_bytes()))
    }
}

/// A writer sequence number. Sequences are contiguous from 1, and 1 is the writer genesis.
pub type Seq = u64;

/// The identity of one operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpId {
    /// Space the operation belongs to.
    pub space: SpaceId,
    /// Writer that sealed it.
    pub writer: WriterId,
    /// Position in the writer's chain.
    pub seq: Seq,
}

/// A replicated table, stable across manifest versions. Ids are never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TableId(pub u16);

/// A field group of a replicated table, `0..MAX_GROUPS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GroupId(u8);

impl GroupId {
    /// Wraps a group id below [`MAX_GROUPS`].
    pub const fn new(id: u8) -> Option<Self> {
        if id < MAX_GROUPS {
            Some(Self(id))
        } else {
            None
        }
    }

    /// Numeric id.
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// A set of group ids as a bit mask, bit `n` for group `n`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GroupMask(pub u64);

impl GroupMask {
    /// The empty set.
    pub const EMPTY: Self = Self(0);

    /// Groups `0..count`.
    pub const fn first(count: u8) -> Self {
        if count >= MAX_GROUPS {
            Self(u64::MAX)
        } else {
            Self((1u64 << count) - 1)
        }
    }

    /// A single group.
    pub const fn of(group: GroupId) -> Self {
        Self(1u64 << group.0)
    }

    /// Whether no group is set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether a group is set.
    pub const fn contains(self, group: GroupId) -> bool {
        self.0 & (1u64 << group.0) != 0
    }

    /// Adds a group.
    pub fn insert(&mut self, group: GroupId) {
        self.0 |= 1u64 << group.0;
    }

    /// Union of two sets.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Number of groups set.
    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    /// Set groups in ascending order.
    pub fn iter(self) -> impl Iterator<Item = GroupId> {
        (0..MAX_GROUPS).filter_map(move |id| {
            let group = GroupId(id);
            self.contains(group).then_some(group)
        })
    }
}

/// A row key that fails validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowKeyError;

impl fmt::Display for RowKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "row key must be non-blank, without NUL, and at most {MAX_ROW_KEY_BYTES} bytes"
        )
    }
}

impl std::error::Error for RowKeyError {}

/// The primary key of a replicated row: UTF-8 of 1 to 128 bytes without NUL that is not only
/// spaces.
///
/// Blank means only U+0020 spaces, matching the SQLite `trim(key) <> ''` rule the vault schema
/// and the value guard triggers apply, so every locally valid key is a valid row key.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RowKey(String);

impl RowKey {
    /// Validates a row key.
    pub fn new(key: impl Into<String>) -> Result<Self, RowKeyError> {
        let key = key.into();
        if key.trim_matches(' ').is_empty() || key.len() > MAX_ROW_KEY_BYTES || key.contains('\0') {
            return Err(RowKeyError);
        }
        Ok(Self(key))
    }

    /// Key text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the key into its text.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Debug for RowKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("RowKey").field(&self.0).finish()
    }
}

impl fmt::Display for RowKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
