//! Bounded big-endian reader and writer used by every sync encoding.

use std::fmt;

/// Bytes that cannot be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecError {
    /// The input ended before a field was complete.
    Truncated,
    /// A length or count exceeds its bound.
    Bounds(&'static str),
    /// A field has a value that the format does not allow.
    Malformed(&'static str),
    /// Bytes remain after the last field.
    Trailing,
}

impl fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => formatter.write_str("encoding is truncated"),
            Self::Bounds(field) => write!(formatter, "{field} exceeds its bound"),
            Self::Malformed(field) => write!(formatter, "{field} is malformed"),
            Self::Trailing => formatter.write_str("encoding has trailing bytes"),
        }
    }
}

impl std::error::Error for CodecError {}

/// Appends big-endian fields to a byte buffer.
#[derive(Debug, Default)]
pub struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    /// Creates a writer with reserved capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(capacity),
        }
    }

    /// Bytes written so far.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether nothing has been written.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Appends one byte.
    pub fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    /// Appends a big-endian u16.
    pub fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    /// Appends a big-endian u32.
    pub fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    /// Appends a big-endian u64.
    pub fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    /// Appends a big-endian i64.
    pub fn i64(&mut self, value: i64) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    /// Appends raw bytes without a length.
    pub fn raw(&mut self, value: &[u8]) {
        self.bytes.extend_from_slice(value);
    }

    /// Appends bytes with a one-byte length. Callers validate the bound first.
    pub fn bytes_u8(&mut self, value: &[u8]) {
        debug_assert!(value.len() <= usize::from(u8::MAX));
        self.u8(value.len() as u8);
        self.raw(value);
    }

    /// Appends bytes with a four-byte length. Callers validate the bound first.
    pub fn bytes_u32(&mut self, value: &[u8]) {
        debug_assert!(u32::try_from(value.len()).is_ok());
        self.u32(value.len() as u32);
        self.raw(value);
    }

    /// Returns the written bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Borrows the written bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Reads big-endian fields from a byte slice, never past its end.
#[derive(Debug)]
pub struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    /// Starts reading at the beginning of a slice.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    /// Bytes consumed so far.
    pub fn position(&self) -> usize {
        self.position
    }

    /// Bytes not yet consumed.
    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    /// Takes a fixed number of bytes.
    pub fn take(&mut self, length: usize) -> Result<&'a [u8], CodecError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or(CodecError::Truncated)?;
        let slice = self
            .bytes
            .get(self.position..end)
            .ok_or(CodecError::Truncated)?;
        self.position = end;
        Ok(slice)
    }

    /// Takes a fixed-size array.
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N], CodecError> {
        let mut output = [0u8; N];
        output.copy_from_slice(self.take(N)?);
        Ok(output)
    }

    /// Reads one byte.
    pub fn u8(&mut self) -> Result<u8, CodecError> {
        Ok(self.take(1)?[0])
    }

    /// Reads a big-endian u16.
    pub fn u16(&mut self) -> Result<u16, CodecError> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    /// Reads a big-endian u32.
    pub fn u32(&mut self) -> Result<u32, CodecError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    /// Reads a big-endian u64.
    pub fn u64(&mut self) -> Result<u64, CodecError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    /// Reads a big-endian i64.
    pub fn i64(&mut self) -> Result<i64, CodecError> {
        Ok(i64::from_be_bytes(self.array()?))
    }

    /// Reads bytes with a one-byte length, refusing lengths above `max`.
    pub fn bytes_u8(&mut self, max: usize, field: &'static str) -> Result<&'a [u8], CodecError> {
        let length = usize::from(self.u8()?);
        if length > max {
            return Err(CodecError::Bounds(field));
        }
        self.take(length)
    }

    /// Reads bytes with a four-byte length, refusing lengths above `max`.
    pub fn bytes_u32(&mut self, max: usize, field: &'static str) -> Result<&'a [u8], CodecError> {
        let length = self.u32()? as usize;
        if length > max {
            return Err(CodecError::Bounds(field));
        }
        self.take(length)
    }

    /// Fails unless every byte was consumed.
    pub fn finish(self) -> Result<(), CodecError> {
        if self.position == self.bytes.len() {
            Ok(())
        } else {
            Err(CodecError::Trailing)
        }
    }
}
