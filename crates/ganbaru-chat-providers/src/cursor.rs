//! Cursor Agent provider over Agent Client Protocol version 1.

mod driver;
mod executable;
mod interactions;
mod normalizer;
mod operations;
mod protocol;
mod session;
mod transport;

#[cfg(test)]
mod tests;

pub use driver::CursorProviderDriver;
