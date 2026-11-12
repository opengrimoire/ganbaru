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
pub(crate) mod replay;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use driver::AcpProviderFlavor;
pub use driver::CursorProviderDriver;
