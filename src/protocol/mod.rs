//! Shared wire protocol and presentation encoding code.

pub mod endpoint;
pub(crate) mod render_ansi;
pub(crate) mod surface_delta;
pub(crate) mod surface_reuse;
pub(crate) mod surface_scroll;
mod wire;

pub use wire::*;

/// A secret-bearing string. It serializes its real value (a newtype over `String`)
/// but renders as `***` in any Debug/Display output, so forwarded token values
/// never leak into logs or error messages. The real value is exposed only via
/// [`SecretString::expose`] at a controlled egress point (the `session.env` API).
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SecretString(String);

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
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the underlying secret. Use only at a controlled egress point.
    pub fn expose(&self) -> &str {
        &self.0
    }
}
