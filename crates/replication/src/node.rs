//! Node identity and cluster state types.

use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Instant;

use crate::config::ClusterConfig;
use crate::version_vector::VersionVector;

/// A unique identifier for a node in the cluster.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(String);

impl NodeId {
    /// Creates a new node ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the node ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for NodeId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for NodeId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Role of a node in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ClusterRole {
    /// Node commits milestones to git.
    Leader = 1,
    /// Node skips milestone commits.
    Follower = 2,
    /// Startup or partition state.
    Unknown = 3,
}

impl ClusterRole {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Leader,
            2 => Self::Follower,
            _ => Self::Unknown,
        }
    }
}

impl fmt::Display for ClusterRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Leader => write!(f, "leader"),
            Self::Follower => write!(f, "follower"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Connection state for a peer.
#[derive(Debug)]
pub struct PeerConnection {
    /// Peer's node ID.
    pub node_id: NodeId,
    /// Peer's address.
    pub addr: String,
    /// Whether currently connected.
    pub connected: bool,
    /// Last successful heartbeat time.
    pub last_heartbeat: Option<Instant>,
    /// Peer's reported role.
    pub role: ClusterRole,
}

impl PeerConnection {
    /// Creates a new peer connection in disconnected state.
    pub fn new(node_id: NodeId, addr: String) -> Self {
        Self {
            node_id,
            addr,
            connected: false,
            last_heartbeat: None,
            role: ClusterRole::Unknown,
        }
    }

    /// Updates the connection as connected with a fresh heartbeat.
    pub fn mark_connected(&mut self, role: ClusterRole) {
        self.connected = true;
        self.last_heartbeat = Some(Instant::now());
        self.role = role;
    }

    /// Updates the connection as disconnected.
    pub fn mark_disconnected(&mut self) {
        self.connected = false;
    }

    /// Checks if the peer should be considered timed out.
    pub fn is_timed_out(&self, timeout: std::time::Duration) -> bool {
        match self.last_heartbeat {
            Some(last) => last.elapsed() > timeout,
            None => true,
        }
    }
}

/// Shared cluster state across the node.
pub struct ClusterState {
    /// This node's ID.
    pub node_id: NodeId,
    /// All known peer node IDs (from config).
    pub known_peers: HashSet<NodeId>,
    /// Connected peer states.
    pub peer_connections: DashMap<NodeId, PeerConnection>,
    /// Version vector tracking highest timestamp seen from each node.
    pub version_vector: RwLock<VersionVector>,
    /// Current cluster role (atomic for lock-free reads).
    role: AtomicU8,
    /// Current leader (if known).
    pub leader_id: RwLock<Option<NodeId>>,
}

impl ClusterState {
    /// Creates a new cluster state from configuration.
    pub fn new(config: &ClusterConfig) -> Self {
        let known_peers: HashSet<NodeId> = config.peers.iter().map(|p| p.node_id.clone()).collect();

        let peer_connections = DashMap::new();
        for peer in &config.peers {
            peer_connections.insert(
                peer.node_id.clone(),
                PeerConnection::new(peer.node_id.clone(), peer.addr.clone()),
            );
        }

        Self {
            node_id: config.node_id.clone(),
            known_peers,
            peer_connections,
            version_vector: RwLock::new(VersionVector::new()),
            role: AtomicU8::new(ClusterRole::Unknown as u8),
            leader_id: RwLock::new(None),
        }
    }

    /// Returns the current cluster role.
    pub fn role(&self) -> ClusterRole {
        ClusterRole::from_u8(self.role.load(Ordering::Acquire))
    }

    /// Sets the cluster role.
    pub fn set_role(&self, role: ClusterRole) {
        self.role.store(role as u8, Ordering::Release);
    }

    /// Returns true if this node is the leader.
    pub fn is_leader(&self) -> bool {
        self.role() == ClusterRole::Leader
    }

    /// Returns the number of currently connected peers.
    pub fn connected_peer_count(&self) -> usize {
        self.peer_connections
            .iter()
            .filter(|entry| entry.value().connected)
            .count()
    }

    /// Returns the total reachable node count (self + connected peers).
    pub fn reachable_count(&self) -> usize {
        1 + self.connected_peer_count()
    }

    /// Computes and updates the cluster role based on current state.
    ///
    /// Leader election algorithm:
    /// 1. Must have majority (quorum) of nodes reachable
    /// 2. Highest node_id among reachable nodes becomes leader
    pub fn compute_role(&self) -> ClusterRole {
        let reachable = self.reachable_count();
        let total = self.known_peers.len() + 1;
        let quorum = total / 2 + 1;

        // Can't be leader without majority
        if reachable < quorum {
            let role = ClusterRole::Follower;
            self.set_role(role);
            return role;
        }

        // Find highest node_id among reachable nodes
        let mut candidates: Vec<NodeId> = self
            .peer_connections
            .iter()
            .filter(|entry| entry.value().connected)
            .map(|entry| entry.key().clone())
            .collect();
        candidates.push(self.node_id.clone());

        let highest = candidates.iter().max().unwrap().clone();

        let role = if highest == self.node_id {
            ClusterRole::Leader
        } else {
            ClusterRole::Follower
        };

        self.set_role(role);

        // Update leader_id
        {
            let mut leader = self.leader_id.write();
            *leader = Some(highest);
        }

        role
    }

    /// Marks a peer as connected with the given role.
    pub fn peer_connected(&self, node_id: &NodeId, role: ClusterRole) {
        if let Some(mut entry) = self.peer_connections.get_mut(node_id) {
            entry.value_mut().mark_connected(role);
        }
    }

    /// Marks a peer as disconnected.
    pub fn peer_disconnected(&self, node_id: &NodeId) {
        if let Some(mut entry) = self.peer_connections.get_mut(node_id) {
            entry.value_mut().mark_disconnected();
        }
    }

    /// Returns connected peer node IDs.
    pub fn connected_peers(&self) -> Vec<NodeId> {
        self.peer_connections
            .iter()
            .filter(|entry| entry.value().connected)
            .map(|entry| entry.key().clone())
            .collect()
    }
}

impl fmt::Debug for ClusterState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClusterState")
            .field("node_id", &self.node_id)
            .field("role", &self.role())
            .field("connected_peers", &self.connected_peer_count())
            .field("known_peers", &self.known_peers.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_id_ordering() {
        let a = NodeId::new("node-a");
        let b = NodeId::new("node-b");
        let c = NodeId::new("node-c");

        assert!(a < b);
        assert!(b < c);
        assert!(c > a);
    }

    #[test]
    fn cluster_role_display() {
        assert_eq!(ClusterRole::Leader.to_string(), "leader");
        assert_eq!(ClusterRole::Follower.to_string(), "follower");
        assert_eq!(ClusterRole::Unknown.to_string(), "unknown");
    }

    #[test]
    fn cluster_state_initialization() {
        let config = ClusterConfig::new("node-a")
            .with_peer("node-b", "b:9401")
            .with_peer("node-c", "c:9401");

        let state = ClusterState::new(&config);

        assert_eq!(state.node_id.as_str(), "node-a");
        assert_eq!(state.known_peers.len(), 2);
        assert_eq!(state.peer_connections.len(), 2);
        assert_eq!(state.role(), ClusterRole::Unknown);
        assert_eq!(state.connected_peer_count(), 0);
    }

    #[test]
    fn leader_election_single_node() {
        let config = ClusterConfig::new("node-a");
        let state = ClusterState::new(&config);

        // Single node is always leader (quorum of 1)
        let role = state.compute_role();
        assert_eq!(role, ClusterRole::Leader);
        assert!(state.is_leader());
    }

    #[test]
    fn leader_election_no_quorum() {
        let config = ClusterConfig::new("node-a")
            .with_peer("node-b", "b:9401")
            .with_peer("node-c", "c:9401");

        let state = ClusterState::new(&config);

        // 3-node cluster, only self reachable (1/3) - no quorum
        let role = state.compute_role();
        assert_eq!(role, ClusterRole::Follower);
    }

    #[test]
    fn leader_election_with_quorum_highest_id() {
        let config = ClusterConfig::new("node-c")
            .with_peer("node-a", "a:9401")
            .with_peer("node-b", "b:9401");

        let state = ClusterState::new(&config);

        // Connect node-a
        state.peer_connected(&NodeId::new("node-a"), ClusterRole::Unknown);

        // 2/3 reachable (quorum), node-c is highest -> leader
        let role = state.compute_role();
        assert_eq!(role, ClusterRole::Leader);
    }

    #[test]
    fn leader_election_with_quorum_not_highest() {
        let config = ClusterConfig::new("node-a")
            .with_peer("node-b", "b:9401")
            .with_peer("node-c", "c:9401");

        let state = ClusterState::new(&config);

        // Connect node-c
        state.peer_connected(&NodeId::new("node-c"), ClusterRole::Unknown);

        // 2/3 reachable (quorum), node-c is highest -> node-a is follower
        let role = state.compute_role();
        assert_eq!(role, ClusterRole::Follower);
    }

    #[test]
    fn peer_connection_lifecycle() {
        let config = ClusterConfig::new("node-a").with_peer("node-b", "b:9401");

        let state = ClusterState::new(&config);

        assert_eq!(state.connected_peer_count(), 0);

        state.peer_connected(&NodeId::new("node-b"), ClusterRole::Follower);
        assert_eq!(state.connected_peer_count(), 1);

        state.peer_disconnected(&NodeId::new("node-b"));
        assert_eq!(state.connected_peer_count(), 0);
    }
}
