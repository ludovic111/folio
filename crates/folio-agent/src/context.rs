//! What the person is looking at when they ask, sent with each request, and the live context
//! sent before every later model step. Both live in `folio_control::harness::context` so the
//! built-in agent and `folio-mcp` give agents the same picture.

pub use folio_control::harness::context::{Context, Glance, context, glance, unframed};
