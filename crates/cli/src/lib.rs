//! Conflux CLI — command-line interface for Conflux.
//!
//! This crate provides the CLI binary for Conflux, including:
//! - Project initialization and configuration
//! - Operation submission (set, import)
//! - State queries (get, diff, log, blame, status)
//! - Milestone management
//! - Environment promotion
//! - Daemon mode (runs the API server)

pub mod actor;
pub mod commands;
pub mod config;
pub mod output;
