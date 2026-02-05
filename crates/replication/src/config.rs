//! Cluster configuration types.

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::node::NodeId;

/// Configuration for a Conflux cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    /// This node's unique identifier.
    pub node_id: NodeId,

    /// Port for replication gRPC service.
    #[serde(default = "default_replication_port")]
    pub replication_port: u16,

    /// Peer nodes in the cluster.
    #[serde(default)]
    pub peers: Vec<PeerConfig>,

    /// Interval between anti-entropy syncs.
    #[serde(default = "default_anti_entropy_interval_secs")]
    pub anti_entropy_interval_secs: u64,

    /// Heartbeat interval.
    #[serde(default = "default_heartbeat_interval_secs")]
    pub heartbeat_interval_secs: u64,

    /// Timeout for considering a peer dead.
    #[serde(default = "default_peer_timeout_secs")]
    pub peer_timeout_secs: u64,
}

/// Configuration for a peer node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerConfig {
    /// Peer's node ID.
    pub node_id: NodeId,

    /// Address to connect to (host:port).
    pub addr: String,
}

fn default_replication_port() -> u16 {
    9401
}

fn default_anti_entropy_interval_secs() -> u64 {
    30
}

fn default_heartbeat_interval_secs() -> u64 {
    5
}

fn default_peer_timeout_secs() -> u64 {
    15
}

impl ClusterConfig {
    /// Creates a new cluster config with the given node ID.
    pub fn new(node_id: impl Into<NodeId>) -> Self {
        Self {
            node_id: node_id.into(),
            replication_port: default_replication_port(),
            peers: Vec::new(),
            anti_entropy_interval_secs: default_anti_entropy_interval_secs(),
            heartbeat_interval_secs: default_heartbeat_interval_secs(),
            peer_timeout_secs: default_peer_timeout_secs(),
        }
    }

    /// Adds a peer to the configuration.
    pub fn with_peer(mut self, node_id: impl Into<NodeId>, addr: impl Into<String>) -> Self {
        self.peers.push(PeerConfig {
            node_id: node_id.into(),
            addr: addr.into(),
        });
        self
    }

    /// Sets the replication port.
    pub fn with_replication_port(mut self, port: u16) -> Self {
        self.replication_port = port;
        self
    }

    /// Returns the anti-entropy interval as a Duration.
    pub fn anti_entropy_interval(&self) -> Duration {
        Duration::from_secs(self.anti_entropy_interval_secs)
    }

    /// Returns the heartbeat interval as a Duration.
    pub fn heartbeat_interval(&self) -> Duration {
        Duration::from_secs(self.heartbeat_interval_secs)
    }

    /// Returns the peer timeout as a Duration.
    pub fn peer_timeout(&self) -> Duration {
        Duration::from_secs(self.peer_timeout_secs)
    }

    /// Returns the total number of nodes in the cluster (including self).
    pub fn cluster_size(&self) -> usize {
        self.peers.len() + 1
    }

    /// Returns the number of nodes required for a majority quorum.
    pub fn quorum_size(&self) -> usize {
        self.cluster_size() / 2 + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cluster_config_defaults() {
        let config = ClusterConfig::new("node-a");
        assert_eq!(config.node_id.as_str(), "node-a");
        assert_eq!(config.replication_port, 9401);
        assert!(config.peers.is_empty());
        assert_eq!(config.anti_entropy_interval_secs, 30);
        assert_eq!(config.heartbeat_interval_secs, 5);
        assert_eq!(config.peer_timeout_secs, 15);
    }

    #[test]
    fn cluster_config_with_peers() {
        let config = ClusterConfig::new("node-a")
            .with_peer("node-b", "node-b.internal:9401")
            .with_peer("node-c", "node-c.internal:9401");

        assert_eq!(config.peers.len(), 2);
        assert_eq!(config.peers[0].node_id.as_str(), "node-b");
        assert_eq!(config.peers[0].addr, "node-b.internal:9401");
    }

    #[test]
    fn quorum_calculation() {
        // 1 node cluster: quorum = 1
        let config = ClusterConfig::new("node-a");
        assert_eq!(config.cluster_size(), 1);
        assert_eq!(config.quorum_size(), 1);

        // 3 node cluster: quorum = 2
        let config = ClusterConfig::new("node-a")
            .with_peer("node-b", "b:9401")
            .with_peer("node-c", "c:9401");
        assert_eq!(config.cluster_size(), 3);
        assert_eq!(config.quorum_size(), 2);

        // 5 node cluster: quorum = 3
        let config = ClusterConfig::new("node-a")
            .with_peer("node-b", "b:9401")
            .with_peer("node-c", "c:9401")
            .with_peer("node-d", "d:9401")
            .with_peer("node-e", "e:9401");
        assert_eq!(config.cluster_size(), 5);
        assert_eq!(config.quorum_size(), 3);
    }

    #[test]
    fn parse_cluster_config_toml() {
        let toml_str = r#"
node_id = "node-a"
replication_port = 9402

[[peers]]
node_id = "node-b"
addr = "node-b.internal:9402"

[[peers]]
node_id = "node-c"
addr = "node-c.internal:9402"
"#;
        let config: ClusterConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.node_id.as_str(), "node-a");
        assert_eq!(config.replication_port, 9402);
        assert_eq!(config.peers.len(), 2);
    }
}
