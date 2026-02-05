//! gRPC replication service implementation.

use std::pin::Pin;
use std::sync::Arc;

use conflux_core::{Clock, Operation};
use conflux_store::SqliteStore;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, Stream, StreamExt};
use tonic::{Request, Response, Status, Streaming};

use crate::node::{ClusterRole, ClusterState};
use crate::proto::{
    replication_service_server::ReplicationService, GetClusterViewRequest, GetClusterViewResponse,
    HeartbeatRequest, HeartbeatResponse, PeerStatus, ReplicateRequest, ReplicateResponse,
    SyncRequest, SyncResponse,
};
use crate::version_vector::VersionVector;
use crate::NodeId;

/// Callback for when a replicated operation is received.
pub type OperationCallback = Arc<dyn Fn(Operation, String) + Send + Sync>;

/// Implementation of the gRPC ReplicationService.
pub struct ReplicationServiceImpl {
    /// Cluster state.
    state: Arc<ClusterState>,
    /// Clock for timestamps.
    clock: Arc<Clock>,
    /// Store for operations (shared with main app).
    store: Arc<Mutex<SqliteStore>>,
    /// Callback for received operations.
    on_operation: Option<OperationCallback>,
}

impl ReplicationServiceImpl {
    /// Creates a new replication service.
    pub fn new(
        state: Arc<ClusterState>,
        clock: Arc<Clock>,
        store: Arc<Mutex<SqliteStore>>,
    ) -> Self {
        Self {
            state,
            clock,
            store,
            on_operation: None,
        }
    }

    /// Sets a callback to be invoked when operations are received.
    pub fn with_operation_callback(mut self, callback: OperationCallback) -> Self {
        self.on_operation = Some(callback);
        self
    }

    fn role_to_proto(&self, role: ClusterRole) -> crate::proto::ClusterRole {
        match role {
            ClusterRole::Leader => crate::proto::ClusterRole::Leader,
            ClusterRole::Follower => crate::proto::ClusterRole::Follower,
            ClusterRole::Unknown => crate::proto::ClusterRole::Unknown,
        }
    }

    fn proto_to_role(&self, role: crate::proto::ClusterRole) -> ClusterRole {
        match role {
            crate::proto::ClusterRole::Leader => ClusterRole::Leader,
            crate::proto::ClusterRole::Follower => ClusterRole::Follower,
            crate::proto::ClusterRole::Unknown | crate::proto::ClusterRole::Unspecified => {
                ClusterRole::Unknown
            }
        }
    }
}

#[tonic::async_trait]
impl ReplicationService for ReplicationServiceImpl {
    type ReplicateOperationsStream =
        Pin<Box<dyn Stream<Item = Result<ReplicateResponse, Status>> + Send>>;

    async fn replicate_operations(
        &self,
        request: Request<Streaming<ReplicateRequest>>,
    ) -> Result<Response<Self::ReplicateOperationsStream>, Status> {
        let mut stream = request.into_inner();
        let (tx, rx) = mpsc::channel(32);

        let state = self.state.clone();
        let clock = self.clock.clone();
        let store = self.store.clone();
        let on_operation = self.on_operation.clone();

        tokio::spawn(async move {
            while let Some(result) = stream.next().await {
                let response = match result {
                    Ok(req) => {
                        let source_node = NodeId::new(&req.source_node_id);

                        // Deserialize operation
                        let op_result: Result<Operation, _> =
                            serde_json::from_str(&req.operation_json);

                        match op_result {
                            Ok(op) => {
                                let op_id = op.id.to_string();

                                // Update clock with incoming timestamp
                                if let Err(e) = clock.update_with_timestamp(&op.timestamp) {
                                    tracing::warn!(
                                        "Clock sync error from {}: {}",
                                        source_node,
                                        e
                                    );
                                }

                                // Check if operation already exists (idempotent)
                                let already_exists = {
                                    let store = store.lock();
                                    store.get_operation(&op.id).is_ok()
                                };

                                if already_exists {
                                    tracing::debug!(
                                        "Operation {} already exists, skipping",
                                        op_id
                                    );
                                    ReplicateResponse {
                                        acknowledged: true,
                                        operation_id: op_id,
                                        error: None,
                                    }
                                } else {
                                    // Store the operation
                                    let store_result = {
                                        let store = store.lock();
                                        store.append_operation(&req.document_id, &op)
                                    };

                                    match store_result {
                                        Ok(()) => {
                                            // Update version vector
                                            {
                                                let mut vv = state.version_vector.write();
                                                vv.update(&source_node, op.timestamp);
                                            }

                                            // Invoke callback to apply to document
                                            if let Some(ref callback) = on_operation {
                                                callback(op, req.document_id);
                                            }

                                            tracing::debug!(
                                                "Replicated operation {} from {}",
                                                op_id,
                                                source_node
                                            );

                                            ReplicateResponse {
                                                acknowledged: true,
                                                operation_id: op_id,
                                                error: None,
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                "Failed to store operation {}: {}",
                                                op_id,
                                                e
                                            );
                                            ReplicateResponse {
                                                acknowledged: false,
                                                operation_id: op_id,
                                                error: Some(e.to_string()),
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to deserialize operation: {}", e);
                                ReplicateResponse {
                                    acknowledged: false,
                                    operation_id: String::new(),
                                    error: Some(format!("deserialization error: {e}")),
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Stream error: {}", e);
                        break;
                    }
                };

                if tx.send(Ok(response)).await.is_err() {
                    break;
                }
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    async fn sync(&self, request: Request<SyncRequest>) -> Result<Response<SyncResponse>, Status> {
        let req = request.into_inner();

        // Parse requester's version vector
        let requester_vv: VersionVector = serde_json::from_str(&req.version_vector_json)
            .map_err(|e| Status::invalid_argument(format!("invalid version vector: {e}")))?;

        // Get our version vector
        let our_vv = self.state.version_vector.read().clone();

        // Find what operations the requester is missing
        let diff = our_vv.diff(&requester_vv);

        // Query operations for nodes where we have newer data
        let mut missing_ops = Vec::new();
        let max_ops = req.max_operations as usize;

        {
            let store = self.store.lock();

            for (node_id, since_ts) in diff {
                if missing_ops.len() >= max_ops {
                    break;
                }

                // Query operations from this node since the timestamp
                // For now, we scan all operations and filter
                // TODO: Add actor-based query to store
                let query = conflux_store::OperationQuery::new(&req.document_id);
                if let Some(ts) = since_ts {
                    // Query operations since ts
                    let query = query.since(&ts);
                    if let Ok(ops) = store.query_operations(&query) {
                        for stored in ops {
                            if missing_ops.len() >= max_ops {
                                break;
                            }
                            // Filter by actor (node_id)
                            if stored.operation.actor.id == node_id.as_str() {
                                let json = serde_json::to_string(&stored.operation)
                                    .map_err(|e| Status::internal(e.to_string()))?;
                                missing_ops.push(json);
                            }
                        }
                    }
                } else {
                    // Node not known to requester, send all ops from this node
                    if let Ok(ops) = store.query_operations(&query) {
                        for stored in ops {
                            if missing_ops.len() >= max_ops {
                                break;
                            }
                            if stored.operation.actor.id == node_id.as_str() {
                                let json = serde_json::to_string(&stored.operation)
                                    .map_err(|e| Status::internal(e.to_string()))?;
                                missing_ops.push(json);
                            }
                        }
                    }
                }
            }
        }

        let our_vv_json =
            serde_json::to_string(&our_vv).map_err(|e| Status::internal(e.to_string()))?;

        let complete = missing_ops.len() < max_ops;

        Ok(Response::new(SyncResponse {
            operations_json: missing_ops,
            version_vector_json: our_vv_json,
            next_cursor: None, // TODO: Implement pagination
            complete,
        }))
    }

    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let req = request.into_inner();

        let peer_node = NodeId::new(&req.node_id);
        let peer_role = self.proto_to_role(req.role());

        // Parse peer's timestamp and update our clock
        let peer_ts: conflux_core::HlcTimestamp = serde_json::from_value(
            serde_json::Value::String(req.timestamp.clone()),
        )
        .map_err(|e| Status::invalid_argument(format!("invalid timestamp: {e}")))?;

        if let Err(e) = self.clock.update_with_timestamp(&peer_ts) {
            tracing::warn!("Clock sync error from {}: {}", peer_node, e);
        }

        // Mark peer as connected
        self.state.peer_connected(&peer_node, peer_role);

        // Recompute our role
        self.state.compute_role();

        let our_ts = self.clock.new_timestamp();

        Ok(Response::new(HeartbeatResponse {
            node_id: self.state.node_id.to_string(),
            timestamp: our_ts.to_string(),
            role: self.role_to_proto(self.state.role()) as i32,
            healthy: true,
        }))
    }

    async fn get_cluster_view(
        &self,
        _request: Request<GetClusterViewRequest>,
    ) -> Result<Response<GetClusterViewResponse>, Status> {
        let leader_id = self.state.leader_id.read().clone();

        let peers: Vec<PeerStatus> = self
            .state
            .peer_connections
            .iter()
            .map(|entry| {
                let conn = entry.value();
                PeerStatus {
                    node_id: conn.node_id.to_string(),
                    connected: conn.connected,
                    last_heartbeat: conn.last_heartbeat.map(|t| format!("{:?}", t.elapsed())),
                    role: self.role_to_proto(conn.role) as i32,
                }
            })
            .collect();

        Ok(Response::new(GetClusterViewResponse {
            node_id: self.state.node_id.to_string(),
            leader_id: leader_id.map(|id| id.to_string()),
            peers,
            role: self.role_to_proto(self.state.role()) as i32,
        }))
    }
}
