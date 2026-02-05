//! Git milestone projection for Conflux.
//!
//! Serializes the resolved config state back to the original file format
//! (YAML, JSON, TOML, KDL, XML, TF/HCL) and commits milestones to a git
//! repository with structured commit messages and causal attribution.
//!
//! # Overview
//!
//! The git crate provides:
//! - **Format serialization** — converts Document entities to YAML, JSON, TOML, KDL, XML, or HCL
//! - **Commit message builder** — creates structured messages with operation attribution
//! - **Milestone projector** — orchestrates the full projection workflow
//!
//! # Example
//!
//! ```rust,no_run
//! use conflux_git::{MilestoneProjector, ProjectorConfig, OutputFormat};
//! use conflux_core::schema_info::SimpleSchemaInfo;
//! use conflux_core::Document;
//!
//! let config = ProjectorConfig::new("/path/to/repo")
//!     .with_format(OutputFormat::Yaml)
//!     .with_author("Conflux", "conflux@example.com");
//!
//! let projector = MilestoneProjector::new(config);
//! let doc = Document::new();
//! let schema = SimpleSchemaInfo::new();
//! let result = projector.project(
//!     &doc,
//!     vec![],
//!     &["production".to_string()],
//!     "Weekly release",
//!     &schema,
//! ).unwrap();
//!
//! println!("Committed: {}", result.commit_sha);
//! ```

pub mod commit;
pub mod error;
pub mod format;
pub mod projector;
pub mod serializer;

pub use commit::CommitBuilder;
pub use error::GitError;
pub use format::{
    serialize, serialize_by_format_id, serialize_document, serialize_with_registry, OutputFile,
    OutputFormat,
};
pub use projector::{MilestoneProjector, MilestoneResult, ProjectorConfig};
pub use serializer::{
    global_registry, register_serializer, HclSerializer, JsonSerializer, KdlSerializer,
    OutputSerializer, OutputSerializerExt, SerializerRegistry, TomlSerializer, XmlSerializer,
    YamlSerializer,
};
