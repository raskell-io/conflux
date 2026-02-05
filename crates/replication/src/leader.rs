//! Leader election and milestone projection guard.

use std::sync::Arc;
use std::time::Duration;

use conflux_core::Clock;
use tokio::time::interval;

use crate::client::PeerManager;
use crate::config::ClusterConfig;
use crate::node::{ClusterRole, ClusterState};

/// Leader election task that maintains heartbeats and computes leadership.
pub struct LeaderElectionTask {
    /// Cluster state.
    state: Arc<ClusterState>,
    /// Peer manager.
    peers: Arc<PeerManager>,
    /// Clock for timestamps.
    clock: Arc<Clock>,
    /// Heartbeat interval.
    heartbeat_interval: Duration,
    /// Peer timeout.
    peer_timeout: Duration,
}

impl LeaderElectionTask {
    /// Creates a new leader election task.
    pub fn new(
        state: Arc<ClusterState>,
        peers: Arc<PeerManager>,
        clock: Arc<Clock>,
        config: &ClusterConfig,
    ) -> Self {
        Self {
            state,
            peers,
            clock,
            heartbeat_interval: config.heartbeat_interval(),
            peer_timeout: config.peer_timeout(),
        }
    }

    /// Runs the leader election loop.
    pub async fn run(self) {
        let mut ticker = interval(self.heartbeat_interval);

        loop {
            ticker.tick().await;

            // Check for timed-out peers
            self.check_peer_timeouts();

            // Send heartbeats to all peers
            let timestamp = self.clock.new_timestamp().to_string();
            self.peers
                .heartbeat_all(&self.state.node_id, self.state.role(), &timestamp)
                .await;

            // Log role changes
            let role = self.state.role();
            tracing::debug!(
                "Node {} role: {}, connected peers: {}/{}",
                self.state.node_id,
                role,
                self.state.connected_peer_count(),
                self.state.known_peers.len()
            );
        }
    }

    /// Checks for peers that have timed out and marks them disconnected.
    fn check_peer_timeouts(&self) {
        for entry in self.state.peer_connections.iter() {
            let conn = entry.value();
            if conn.connected && conn.is_timed_out(self.peer_timeout) {
                tracing::warn!("Peer {} timed out", conn.node_id);
                self.state.peer_disconnected(&conn.node_id);
            }
        }
    }
}

/// Guard for leader-only operations like milestone projection.
///
/// Returns `Ok(())` if this node is the leader, `Err` otherwise.
pub fn require_leader(state: &ClusterState) -> Result<(), LeadershipError> {
    if state.is_leader() {
        Ok(())
    } else {
        let leader = state.leader_id.read().clone();
        Err(LeadershipError::NotLeader {
            current_role: state.role(),
            leader_id: leader,
        })
    }
}

/// Error when a leader-only operation is attempted on a non-leader node.
#[derive(Debug)]
pub enum LeadershipError {
    /// This node is not the leader.
    NotLeader {
        current_role: ClusterRole,
        leader_id: Option<crate::NodeId>,
    },
}

impl std::fmt::Display for LeadershipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotLeader {
                current_role,
                leader_id,
            } => {
                write!(f, "node is {} (not leader)", current_role)?;
                if let Some(id) = leader_id {
                    write!(f, ", current leader is {}", id)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for LeadershipError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ClusterConfig;

    #[test]
    fn require_leader_when_leader() {
        let config = ClusterConfig::new("node-a");
        let state = ClusterState::new(&config);
        state.compute_role(); // Single node becomes leader

        assert!(require_leader(&state).is_ok());
    }

    #[test]
    fn require_leader_when_not_leader() {
        let config = ClusterConfig::new("node-a")
            .with_peer("node-b", "b:9401")
            .with_peer("node-c", "c:9401");

        let state = ClusterState::new(&config);
        // No peers connected, so no quorum -> follower
        state.compute_role();

        let result = require_leader(&state);
        assert!(result.is_err());

        if let Err(LeadershipError::NotLeader { current_role, .. }) = result {
            assert_eq!(current_role, ClusterRole::Follower);
        } else {
            panic!("expected NotLeader error");
        }
    }
}
