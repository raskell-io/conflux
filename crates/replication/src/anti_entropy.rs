//! Anti-entropy background sync for gap detection and repair.

use std::sync::Arc;
use std::time::Duration;

use conflux_core::{Clock, Operation};
use conflux_store::SqliteStore;
use parking_lot::Mutex;
use tokio::time::interval;

use crate::client::PeerManager;
use crate::config::ClusterConfig;
use crate::error::ReplicationError;
use crate::node::{ClusterState, NodeId};
use crate::version_vector::VersionVector;

/// Callback for applying synced operations.
pub type ApplyOperationFn = Arc<dyn Fn(Operation, String) + Send + Sync>;

/// Anti-entropy sync task that periodically syncs with peers.
pub struct AntiEntropyTask {
    /// Cluster state.
    state: Arc<ClusterState>,
    /// Peer manager.
    peers: Arc<PeerManager>,
    /// Store for operations.
    store: Arc<Mutex<SqliteStore>>,
    /// Clock for timestamps.
    clock: Arc<Clock>,
    /// Document ID to sync.
    document_id: String,
    /// Sync interval.
    interval: Duration,
    /// Max operations per sync request.
    max_operations: u32,
    /// Callback to apply received operations.
    apply_fn: Option<ApplyOperationFn>,
}

impl AntiEntropyTask {
    /// Creates a new anti-entropy task.
    pub fn new(
        state: Arc<ClusterState>,
        peers: Arc<PeerManager>,
        store: Arc<Mutex<SqliteStore>>,
        clock: Arc<Clock>,
        document_id: impl Into<String>,
        config: &ClusterConfig,
    ) -> Self {
        Self {
            state,
            peers,
            store,
            clock,
            document_id: document_id.into(),
            interval: config.anti_entropy_interval(),
            max_operations: 1000,
            apply_fn: None,
        }
    }

    /// Sets the callback for applying received operations.
    pub fn with_apply_fn(mut self, apply_fn: ApplyOperationFn) -> Self {
        self.apply_fn = Some(apply_fn);
        self
    }

    /// Runs the anti-entropy loop.
    pub async fn run(self) {
        let mut ticker = interval(self.interval);

        loop {
            ticker.tick().await;

            // Pick a random connected peer to sync with
            let connected = self.state.connected_peers();
            if connected.is_empty() {
                tracing::debug!("No connected peers for anti-entropy sync");
                continue;
            }

            // Simple round-robin - just pick the first one
            // TODO: Randomize for better distribution
            let peer_id = &connected[0];

            if let Err(e) = self.sync_with_peer(peer_id).await {
                tracing::warn!("Anti-entropy sync with {} failed: {}", peer_id, e);
            }
        }
    }

    /// Performs a sync with a specific peer.
    async fn sync_with_peer(&self, peer_id: &NodeId) -> Result<(), ReplicationError> {
        let mut client = self.peers.get(peer_id).ok_or_else(|| {
            ReplicationError::SyncFailed {
                peer: peer_id.to_string(),
                reason: "peer not found".to_string(),
            }
        })?;

        // Get our version vector
        let our_vv = self.state.version_vector.read().clone();

        // Request sync
        let (operations, peer_vv) = client
            .sync(&self.document_id, &our_vv, self.max_operations)
            .await?;

        if operations.is_empty() {
            tracing::debug!("Anti-entropy sync with {}: no missing operations", peer_id);
        } else {
            tracing::info!(
                "Anti-entropy sync with {}: received {} operations",
                peer_id,
                operations.len()
            );

            // Apply received operations
            for op in operations {
                // Update clock
                if let Err(e) = self.clock.update_with_timestamp(&op.timestamp) {
                    tracing::warn!("Clock sync error: {}", e);
                }

                // Check if operation already exists
                let exists = {
                    let store = self.store.lock();
                    store.get_operation(&op.id).is_ok()
                };

                if exists {
                    tracing::debug!("Operation {} already exists, skipping", op.id);
                    continue;
                }

                // Store operation
                {
                    let store = self.store.lock();
                    if let Err(e) = store.append_operation(&self.document_id, &op) {
                        tracing::error!("Failed to store synced operation {}: {}", op.id, e);
                        continue;
                    }
                }

                // Apply to document
                if let Some(ref apply_fn) = self.apply_fn {
                    apply_fn(op.clone(), self.document_id.clone());
                }

                // Update version vector
                let actor_node = NodeId::new(&op.actor.id);
                {
                    let mut vv = self.state.version_vector.write();
                    vv.update(&actor_node, op.timestamp);
                }
            }
        }

        // Merge peer's version vector into ours
        {
            let mut vv = self.state.version_vector.write();
            vv.merge(&peer_vv);
        }

        Ok(())
    }
}

/// Builds the version vector from the operation log.
///
/// Scans all operations and tracks the highest timestamp from each actor.
pub fn build_version_vector_from_store(
    store: &SqliteStore,
    document_id: &str,
) -> Result<VersionVector, ReplicationError> {
    let query = conflux_store::OperationQuery::new(document_id);
    let operations = store.query_operations(&query)?;

    let mut vv = VersionVector::new();
    for stored in operations {
        let node_id = NodeId::new(&stored.operation.actor.id);
        vv.update(&node_id, stored.operation.timestamp);
    }

    Ok(vv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use conflux_core::{ActorClass, ActorId, Clock, FieldValue};

    fn make_operation(clock: &Clock, actor_id: &str) -> Operation {
        let actor = ActorId::new(actor_id, ActorClass::Operator);
        Operation::set_field(
            "test.entity",
            "field",
            FieldValue::Int(42),
            &actor,
            clock.new_timestamp(),
        )
    }

    #[test]
    fn build_version_vector() {
        let store = SqliteStore::open_in_memory().unwrap();
        let clock = Clock::new();

        // Add operations from different "nodes" (using actor.id as node)
        let op1 = make_operation(&clock, "node-a");
        let op2 = make_operation(&clock, "node-b");
        let op3 = make_operation(&clock, "node-a");

        store.append_operation("doc-1", &op1).unwrap();
        store.append_operation("doc-1", &op2).unwrap();
        store.append_operation("doc-1", &op3).unwrap();

        let vv = build_version_vector_from_store(&store, "doc-1").unwrap();

        // node-a should have op3's timestamp (highest)
        assert_eq!(vv.get(&NodeId::new("node-a")), Some(&op3.timestamp));
        // node-b should have op2's timestamp
        assert_eq!(vv.get(&NodeId::new("node-b")), Some(&op2.timestamp));
    }
}
