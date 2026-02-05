//! Application state shared across handlers.

use conflux_core::{Clock, Document};
use conflux_git::{MilestoneProjector, ProjectorConfig};
use conflux_schema::Schema;
use conflux_store::SqliteStore;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    /// The current document state.
    pub document: Arc<RwLock<Document>>,
    /// Hybrid logical clock for timestamps.
    pub clock: Arc<Clock>,
    /// Schema for validation and merge strategies.
    pub schema: Arc<Schema>,
    /// Persistent store for operations and snapshots (wrapped in Mutex for thread safety).
    pub store: Arc<Mutex<SqliteStore>>,
    /// Git milestone projector (optional).
    pub projector: Option<Arc<MilestoneProjector>>,
    /// Document ID for this instance.
    pub document_id: String,
}

impl AppState {
    /// Creates a new application state.
    pub fn new(schema: Schema, store: SqliteStore, document_id: impl Into<String>) -> Self {
        Self {
            document: Arc::new(RwLock::new(Document::new())),
            clock: Arc::new(Clock::new()),
            schema: Arc::new(schema),
            store: Arc::new(Mutex::new(store)),
            projector: None,
            document_id: document_id.into(),
        }
    }

    /// Configures git milestone projection.
    pub fn with_git_repo(mut self, repo_path: impl Into<PathBuf>) -> Self {
        let config = ProjectorConfig::new(repo_path);
        self.projector = Some(Arc::new(MilestoneProjector::new(config)));
        self
    }
}
