//! Shared wire protocol and presentation encoding code.

pub mod abi;
pub(crate) mod render_ansi;
mod wire;
#[cfg(all(test, not(windows)))]
mod wire_fixtures;

pub use wire::*;

use serde::{Deserialize, Serialize};

/// A secret-bearing string. It serializes its real value over the wire (a
/// newtype over `String`) but renders as `***` in any Debug/Display output, so
/// forwarded token values can never leak into logs or error messages. The real
/// value is exposed only to code via [`SecretString::expose`] and to the
/// controlled `session.env` API egress (JSON delivered to the querying plugin).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretString(pub String);

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("***")
    }
}

impl std::fmt::Display for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("***")
    }
}

impl SecretString {
    /// Wraps a secret value, redacting it from all Debug/Display output.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    /// Returns the underlying secret. Use only at a controlled egress point.
    pub fn expose(&self) -> &str {
        &self.0
    }
}
