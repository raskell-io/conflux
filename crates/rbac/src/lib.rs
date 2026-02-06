//! Role-based access control for Conflux.
//!
//! This crate provides RBAC (Role-Based Access Control) for Conflux operations.
//! It supports:
//!
//! - **Role definitions** with inheritance
//! - **Permission patterns** for granular access control
//! - **Actor-to-role assignments** including pattern matching
//! - **Environment-scoped permissions** (e.g., write in dev, read-only in prod)
//!
//! # Configuration
//!
//! RBAC is configured via a TOML file:
//!
//! ```toml
//! [rbac]
//! version = "1.0"
//! default_authenticated = ["viewer"]
//! default_anonymous = []
//!
//! [role.viewer]
//! description = "Read-only access"
//! permissions = [
//!     { actions = ["read"], resource = "*" },
//! ]
//!
//! [role.developer]
//! inherits = ["viewer"]
//! permissions = [
//!     { actions = ["write", "create", "delete"], resource = "*@development" },
//! ]
//!
//! [role.admin]
//! permissions = [
//!     { actions = ["*"], resource = "*" },
//! ]
//!
//! [[assignment]]
//! actor = "alice"
//! roles = ["admin"]
//!
//! [[assignment]]
//! actor_pattern = "ci-*"
//! actor_class = "pipeline"
//! roles = ["developer"]
//! ```
//!
//! # Resource Pattern Syntax
//!
//! Resources are matched using patterns with the following syntax:
//!
//! ```text
//! <entity_type>[/<entity_id>][.<field>][@<environment>]
//! ```
//!
//! Examples:
//! - `*` — matches all resources
//! - `service` — matches all service entities
//! - `service/api-gateway` — matches specific service
//! - `service.replicas` — matches replicas field on all services
//! - `*@production` — matches all resources in production
//! - `service/api-*` — matches services with ID prefix
//!
//! # Usage
//!
//! ```rust,no_run
//! use conflux_rbac::{RbacConfig, RbacAuthorizer, Authorizer, AuthzContext, Action, Resource};
//! use conflux_core::{ActorId, ActorClass};
//!
//! // Load configuration
//! let config = RbacConfig::from_file("rbac.toml".as_ref()).unwrap();
//! let authorizer = RbacAuthorizer::new(config);
//!
//! // Check authorization
//! let actor = ActorId::new("alice", ActorClass::Human);
//! let ctx = AuthzContext::new(
//!     actor,
//!     Action::Write,
//!     Resource::entity("service", "api-gateway").in_env("production"),
//! );
//!
//! match authorizer.authorize(&ctx) {
//!     conflux_rbac::AuthzResult::Allowed => println!("Access granted"),
//!     conflux_rbac::AuthzResult::Denied(reason) => println!("Access denied: {}", reason),
//! }
//! ```

pub mod authorizer;
pub mod config;
pub mod error;
pub mod resolver;
pub mod types;

pub use authorizer::{AuthzContext, AuthzResult, Authorizer, ChainAuthorizer, DenialReason, RbacAuthorizer};
pub use config::RbacConfig;
pub use error::RbacError;
pub use resolver::RoleResolver;
pub use types::{Action, Resource, ResourcePattern};
