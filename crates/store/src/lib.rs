//! Persistent state storage for Conflux.
//!
//! Stores the operation log, resolved document state, and actor metadata.
//! Uses SQLite for single-node deployments with a clean abstraction layer
//! for future storage backends.
