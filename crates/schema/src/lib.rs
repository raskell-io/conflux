//! Schema definition language for Conflux.
//!
//! Defines config structure, field types, and per-field merge strategies.
//! Schemas are written in TOML and describe entities, fields, merge behavior,
//! and environment overlay rules.
//!
//! # Usage
//!
//! ```rust,no_run
//! use conflux_schema::Schema;
//!
//! let schema = Schema::from_file("schema.toml").unwrap();
//! schema.validate().unwrap();
//! ```

pub mod error;
pub mod parser;
pub mod types;
pub mod validation;

mod schema_info_impl;

pub use error::SchemaError;
pub use schema_info_impl::SchemaInfoAdapter;
pub use types::{
    ChildDef, ConflictMode, EntityDef, EnvironmentsDef, FieldDef, FieldType, OverlayDef, Schema,
};
