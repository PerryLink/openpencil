//! Compatibility surface for the Hosted runtime shared by desktop and mobile.
//!
//! The daemon-only request parser remains here; identity, catalog discovery,
//! transport and cancellation live in the mobile-safe `op-hosted-models` crate.

pub use op_hosted_models::*;
pub(crate) mod request;

#[cfg(test)]
mod tests;
