//! gRPC client for connecting to peer nodes.

use std::sync::Arc;
use std::time::Duration;

use conflux_core::Operation;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Channel;

use crate::error::ReplicationError;
use crate::node::{ClusterRole, ClusterState, NodeId};
use crate::proto::{
    replication_service_client::ReplicationServiceClient, GetClusterViewRequest, HeartbeatRequest,
    ReplicateRequest, SyncRequest,
};
use crate::version_vector::VersionVector;

/// Client for communicating with a peer node.
pub struct PeerClient {
    /// Peer's node ID.
    node_id: NodeId,
    /// Peer's address.
    addr: String,
    /// gRPC client (lazy initialized).
    client: Option<ReplicationServiceClient<Channel>>,
    /// Sender for operation replication stream.
    op_sender: Option<mpsc::Sender<ReplicateRequest>>,
}

impl PeerClient {
    /// Creates a new peer client.
    pub fn new(node_id: NodeId, addr: String) -> Self {
        Self {
            node_id,
            addr,
            client: None,
            op_sender: None,
        }
    }

    /// Returns the peer's node ID.
    pub fn node_id(&self) -> &NodeId {
        &self.node_id
    }

    /// Returns true if connected.
    pub fn is_connected(&self) -> bool {
        self.client.is_some()
    }

    /// Connects to the peer node.
    pub async fn connect(&mut self) -> Result<(), ReplicationError> {
        let endpoint = format!("http://{}", self.addr);

        let channel = Channel::from_shared(endpoint.clone())
            .map_err(|e| ReplicationError::ConnectionFailed {
                peer: self.node_id.to_string(),
                reason: e.to_string(),
            })?
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .connect()
            .await
            .map_err(|e| ReplicationError::ConnectionFailed {
                peer: self.node_id.to_string(),
                reason: e.to_string(),
            })?;

        self.client = Some(ReplicationServiceClient::new(channel));
        tracing::info!("Connected to peer {}", self.node_id);

        Ok(())
    }

    /// Disconnects from the peer.
    pub fn disconnect(&mut self) {
        self.client = None;
        self.op_sender = None;
        tracing::info!("Disconnected from peer {}", self.node_id);
    }

    /// Sends a heartbeat to the peer.
    pub async fn heartbeat(
        &mut self,
        our_node_id: &NodeId,
        our_role: ClusterRole,
        timestamp: &str,
    ) -> Result<(NodeId, ClusterRole), ReplicationError> {
        let client = self.client.as_mut().ok_or_else(|| {
            ReplicationError::ConnectionFailed {
                peer: self.node_id.to_string(),
                reason: "not connected".to_string(),
            }
        })?;

        let request = HeartbeatRequest {
            node_id: our_node_id.to_string(),
            timestamp: timestamp.to_string(),
            role: role_to_proto(our_role) as i32,
        };

        let response = client.heartbeat(request).await?.into_inner();

        let peer_role = proto_to_role(response.role());

        Ok((NodeId::new(response.node_id), peer_role))
    }

    /// Sends an operation to the peer for replication.
    pub async fn replicate_operation(
        &mut self,
        operation: &Operation,
        document_id: &str,
        source_node_id: &NodeId,
    ) -> Result<bool, ReplicationError> {
        let client = self.client.as_mut().ok_or_else(|| {
            ReplicationError::ConnectionFailed {
                peer: self.node_id.to_string(),
                reason: "not connected".to_string(),
            }
        })?;

        let op_json = serde_json::to_string(operation)?;

        let request = ReplicateRequest {
            operation_json: op_json,
            document_id: document_id.to_string(),
            source_node_id: source_node_id.to_string(),
        };

        // Use a one-shot stream for now
        let (tx, rx) = mpsc::channel(1);
        tx.send(request).await.map_err(|_| {
            ReplicationError::ConnectionFailed {
                peer: self.node_id.to_string(),
                reason: "channel closed".to_string(),
            }
        })?;
        drop(tx);

        let stream = ReceiverStream::new(rx);
        let mut response_stream = client.replicate_operations(stream).await?.into_inner();

        // Get first response
        if let Some(result) = tokio_stream::StreamExt::next(&mut response_stream).await {
            let response = result?;
            if let Some(error) = response.error {
                tracing::warn!(
                    "Peer {} rejected operation: {}",
                    self.node_id,
                    error
                );
                return Ok(false);
            }
            return Ok(response.acknowledged);
        }

        Ok(false)
    }

    /// Performs anti-entropy sync with the peer.
    pub async fn sync(
        &mut self,
        document_id: &str,
        our_vv: &VersionVector,
        max_operations: u32,
    ) -> Result<(Vec<Operation>, VersionVector), ReplicationError> {
        let client = self.client.as_mut().ok_or_else(|| {
            ReplicationError::ConnectionFailed {
                peer: self.node_id.to_string(),
                reason: "not connected".to_string(),
            }
        })?;

        let vv_json = serde_json::to_string(our_vv)?;

        let request = SyncRequest {
            document_id: document_id.to_string(),
            version_vector_json: vv_json,
            max_operations,
            cursor: None,
        };

        let response = client.sync(request).await?.into_inner();

        // Parse operations
        let mut operations = Vec::new();
        for op_json in &response.operations_json {
            let op: Operation = serde_json::from_str(op_json)?;
            operations.push(op);
        }

        // Parse peer's version vector
        let peer_vv: VersionVector = serde_json::from_str(&response.version_vector_json)?;

        Ok((operations, peer_vv))
    }

    /// Gets the cluster view from the peer.
    pub async fn get_cluster_view(
        &mut self,
        our_node_id: &NodeId,
    ) -> Result<crate::proto::GetClusterViewResponse, ReplicationError> {
        let client = self.client.as_mut().ok_or_else(|| {
            ReplicationError::ConnectionFailed {
                peer: self.node_id.to_string(),
                reason: "not connected".to_string(),
            }
        })?;

        let request = GetClusterViewRequest {
            node_id: our_node_id.to_string(),
        };

        let response = client.get_cluster_view(request).await?.into_inner();
        Ok(response)
    }
}

fn role_to_proto(role: ClusterRole) -> crate::proto::ClusterRole {
    match role {
        ClusterRole::Leader => crate::proto::ClusterRole::Leader,
        ClusterRole::Follower => crate::proto::ClusterRole::Follower,
        ClusterRole::Unknown => crate::proto::ClusterRole::Unknown,
    }
}

fn proto_to_role(role: crate::proto::ClusterRole) -> ClusterRole {
    match role {
        crate::proto::ClusterRole::Leader => ClusterRole::Leader,
        crate::proto::ClusterRole::Follower => ClusterRole::Follower,
        crate::proto::ClusterRole::Unknown | crate::proto::ClusterRole::Unspecified => {
            ClusterRole::Unknown
        }
    }
}

/// Configuration for reconnection with exponential backoff.
#[derive(Debug, Clone)]
pub struct ReconnectConfig {
    /// Initial delay before first reconnect attempt.
    pub initial_delay: Duration,
    /// Maximum delay between reconnect attempts.
    pub max_delay: Duration,
    /// Multiplier for exponential backoff.
    pub multiplier: f64,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            multiplier: 2.0,
        }
    }
}

/// Manager for all peer connections.
pub struct PeerManager {
    /// Our node's state.
    state: Arc<ClusterState>,
    /// Peer clients indexed by node ID.
    clients: dashmap::DashMap<NodeId, PeerClient>,
    /// Reconnection configuration.
    reconnect_config: ReconnectConfig,
    /// Current backoff delay per peer.
    backoff_delays: dashmap::DashMap<NodeId, Duration>,
}

impl PeerManager {
    /// Creates a new peer manager.
    pub fn new(state: Arc<ClusterState>) -> Self {
        let clients = dashmap::DashMap::new();
        let backoff_delays = dashmap::DashMap::new();

        // Initialize clients for all known peers
        for entry in state.peer_connections.iter() {
            let conn = entry.value();
            clients.insert(
                conn.node_id.clone(),
                PeerClient::new(conn.node_id.clone(), conn.addr.clone()),
            );
        }

        Self {
            state,
            clients,
            reconnect_config: ReconnectConfig::default(),
            backoff_delays,
        }
    }

    /// Creates a new peer manager with custom reconnection config.
    pub fn with_reconnect_config(mut self, config: ReconnectConfig) -> Self {
        self.reconnect_config = config;
        self
    }

    /// Connects to all peers.
    pub async fn connect_all(&self) {
        let peers: Vec<_> = self.clients.iter().map(|e| e.key().clone()).collect();

        for node_id in peers {
            if let Some(mut client) = self.clients.get_mut(&node_id) {
                if let Err(e) = client.connect().await {
                    tracing::warn!("Failed to connect to {}: {}", node_id, e);
                    self.state.peer_disconnected(&node_id);
                    // Initialize backoff for failed connection
                    self.backoff_delays
                        .insert(node_id, self.reconnect_config.initial_delay);
                } else {
                    // Clear backoff on successful connection
                    self.backoff_delays.remove(&node_id);
                }
            }
        }
    }

    /// Attempts to reconnect to disconnected peers with exponential backoff.
    pub async fn reconnect_disconnected(&self) {
        let peers: Vec<_> = self.clients.iter().map(|e| e.key().clone()).collect();

        for node_id in peers {
            if let Some(mut client) = self.clients.get_mut(&node_id) {
                if !client.is_connected() {
                    // Get current backoff delay
                    let delay = self
                        .backoff_delays
                        .get(&node_id)
                        .map(|d| *d)
                        .unwrap_or(self.reconnect_config.initial_delay);

                    tracing::debug!(
                        "Attempting to reconnect to {} (backoff: {:?})",
                        node_id,
                        delay
                    );

                    if let Err(e) = client.connect().await {
                        tracing::warn!("Reconnect to {} failed: {}", node_id, e);

                        // Increase backoff delay
                        let new_delay = Duration::from_secs_f64(
                            (delay.as_secs_f64() * self.reconnect_config.multiplier)
                                .min(self.reconnect_config.max_delay.as_secs_f64()),
                        );
                        self.backoff_delays.insert(node_id.clone(), new_delay);
                    } else {
                        tracing::info!("Reconnected to {}", node_id);
                        // Clear backoff on successful connection
                        self.backoff_delays.remove(&node_id);
                        self.state.peer_connected(&node_id, ClusterRole::Unknown);
                    }
                }
            }
        }
    }

    /// Gets a client for a specific peer.
    pub fn get(&self, node_id: &NodeId) -> Option<dashmap::mapref::one::RefMut<'_, NodeId, PeerClient>> {
        self.clients.get_mut(node_id)
    }

    /// Broadcasts an operation to all connected peers.
    pub async fn broadcast_operation(
        &self,
        operation: &Operation,
        document_id: &str,
        source_node_id: &NodeId,
    ) -> Vec<(NodeId, Result<bool, ReplicationError>)> {
        let peers: Vec<_> = self.clients.iter().map(|e| e.key().clone()).collect();
        let mut results = Vec::new();

        for node_id in peers {
            if let Some(mut client) = self.clients.get_mut(&node_id) {
                if client.is_connected() {
                    let result = client
                        .replicate_operation(operation, document_id, source_node_id)
                        .await;
                    results.push((node_id, result));
                }
            }
        }

        results
    }

    /// Sends heartbeats to all peers.
    pub async fn heartbeat_all(
        &self,
        our_node_id: &NodeId,
        our_role: ClusterRole,
        timestamp: &str,
    ) {
        let peers: Vec<_> = self.clients.iter().map(|e| e.key().clone()).collect();

        for node_id in peers {
            if let Some(mut client) = self.clients.get_mut(&node_id) {
                match client.heartbeat(our_node_id, our_role, timestamp).await {
                    Ok((_, peer_role)) => {
                        self.state.peer_connected(&node_id, peer_role);
                    }
                    Err(e) => {
                        tracing::warn!("Heartbeat to {} failed: {}", node_id, e);
                        self.state.peer_disconnected(&node_id);
                    }
                }
            }
        }

        // Recompute role after heartbeats
        self.state.compute_role();
    }

    /// Returns the number of connected peers.
    pub fn connected_count(&self) -> usize {
        self.clients
            .iter()
            .filter(|e| e.value().is_connected())
            .count()
    }

    /// Returns the total number of peers.
    pub fn total_peers(&self) -> usize {
        self.clients.len()
    }
}

/// Background task for periodic reconnection attempts.
pub struct ReconnectTask {
    peer_manager: Arc<PeerManager>,
    interval: Duration,
}

impl ReconnectTask {
    /// Creates a new reconnect task.
    pub fn new(peer_manager: Arc<PeerManager>, interval: Duration) -> Self {
        Self {
            peer_manager,
            interval,
        }
    }

    /// Runs the reconnect loop.
    pub async fn run(self) {
        let mut ticker = tokio::time::interval(self.interval);

        loop {
            ticker.tick().await;

            let connected = self.peer_manager.connected_count();
            let total = self.peer_manager.total_peers();

            if connected < total {
                tracing::debug!(
                    "Reconnect check: {}/{} peers connected",
                    connected,
                    total
                );
                self.peer_manager.reconnect_disconnected().await;
            }
        }
    }
}
