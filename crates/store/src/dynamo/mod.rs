//! DynamoDB-backed async storage for Conflux.
//!
//! This module provides a DynamoDB implementation of the [`AsyncStore`](crate::AsyncStore) trait
//! using a single-table design optimized for serverless deployments.
//!
//! # Table Design
//!
//! Uses a single-table design with the following key structure:
//!
//! | PK | SK | Type |
//! |----|----|----|
//! | `DOC#<doc_id>` | `OP#<hlc>#<op_id>` | Operation |
//! | `DOC#<doc_id>` | `SNAP#<hlc>#<id>` | Snapshot |
//! | `DOC#<doc_id>` | `MILE#<created>#<id>` | Milestone |
//! | `DOC#<doc_id>` | `VV#` | Version Vector |
//!
//! # GSIs
//!
//! - **GSI1**: `gsi1_pk=OP#<op_id>` — Operation lookup by ID
//! - **GSI2**: `gsi2_pk=ACTOR#<doc>#<actor>`, `gsi2_sk=<hlc>` — Operations by actor
//!
//! # Example
//!
//! ```ignore
//! use conflux_store::DynamoStore;
//!
//! let store = DynamoStore::new("conflux-store").await?;
//! store.create_table_if_not_exists().await?;
//! store.append_operation("doc-1", &op).await?;
//! ```

mod keys;
mod store;

pub use store::DynamoStore;
